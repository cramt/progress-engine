//! JavaScript acceptance criteria, evaluated against the exact engine.
//!
//! Criteria are real JavaScript so that combining requirements is ordinary code
//! rather than a bespoke grammar. The API they get is deliberately narrow: a
//! criterion may ask `t(n).count(query)` and nothing else. It never sees the
//! cards.
//!
//! That narrowness is the whole trick. A criterion that depends only on counts
//! is a pure function of the composition, so it can be evaluated once per
//! possible composition rather than once per simulated hand — exact rather than
//! sampled, and faster besides.

use deno_core::{extension, op2, v8, JsRuntime, OpState, RuntimeOptions};
use pe_criteria::{Criterion, Evaluator, PathView};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JsError {
    #[error("loading criteria: {0}")]
    Load(String),
    #[error("evaluating criteria: {0}")]
    Eval(String),
    #[error("criteria file registered no criterion() calls")]
    NoCriteria,
}

/// Counts for the path currently being evaluated, indexed
/// `[checkpoint][query]`, plus any query JavaScript asked for that Rust has not
/// grouped yet.
#[derive(Default)]
struct CountTable {
    /// Deepest `t(n)` any criterion asked for, so the run models exactly as many
    /// turns as the file actually cares about.
    max_checkpoint: u32,
    queries: Vec<String>,
    counts: Vec<Vec<u32>>,
    discovered: Vec<String>,
}

#[op2(fast)]
#[smi]
fn op_pe_count(state: &mut OpState, #[smi] checkpoint: u32, #[string] query: String) -> u32 {
    let table = state.borrow_mut::<CountTable>();
    table.max_checkpoint = table.max_checkpoint.max(checkpoint);
    match table.queries.iter().position(|q| *q == query) {
        Some(idx) => table
            .counts
            .get(checkpoint as usize)
            .and_then(|row| row.get(idx))
            .copied()
            .unwrap_or(0),
        None => {
            // A query nobody has grouped by yet. Record it and answer 0; the
            // driver notices, regroups and starts over, so the answer this run
            // produced is discarded rather than trusted.
            if !table.discovered.contains(&query) {
                table.discovered.push(query);
            }
            0
        }
    }
}

extension!(
    pe_ext,
    ops = [op_pe_count],
    state = |state: &mut OpState| state.put(CountTable::default()),
);

pub struct Criteria {
    runtime: JsRuntime,
    evaluate: v8::Global<v8::Function>,
    criteria: Vec<Criterion>,
}

impl Criteria {
    /// Load a criteria file.
    pub fn load(source: String) -> Result<Self, JsError> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![pe_ext::init()],
            ..Default::default()
        });

        runtime
            .execute_script("[pe:bootstrap]", include_str!("bootstrap.js"))
            .map_err(|e| JsError::Load(e.to_string()))?;
        runtime
            .execute_script("[criteria]", source)
            .map_err(|e| JsError::Load(e.to_string()))?;

        let criteria = read_meta(&mut runtime)?;
        if criteria.is_empty() {
            return Err(JsError::NoCriteria);
        }
        let evaluate = global_function(&mut runtime, "__evaluate")?;

        Ok(Criteria {
            runtime,
            evaluate,
            criteria,
        })
    }

    pub fn criteria(&self) -> &[Criterion] {
        &self.criteria
    }

    /// Queries the criteria asked about that were not in the current grouping.
    pub fn take_discovered(&mut self) -> Vec<String> {
        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        std::mem::take(&mut state.borrow_mut::<CountTable>().discovered)
    }

    /// Tell the runtime which queries the grouping now covers.
    pub fn set_queries(&mut self, queries: Vec<String>) {
        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let table = state.borrow_mut::<CountTable>();
        table.queries = queries;
        table.discovered.clear();
    }

    fn set_counts(&mut self, counts: Vec<Vec<u32>>) {
        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        state.borrow_mut::<CountTable>().counts = counts;
    }

    /// Run every criterion once with all counts zero, purely to discover which
    /// queries they reference.
    pub fn probe(&mut self) -> Result<(), JsError> {
        self.set_counts(vec![vec![]; 8]);
        self.call_evaluate().map(|_| ())
    }

    fn call_evaluate(&mut self) -> Result<Vec<bool>, JsError> {
        let evaluate = self.evaluate.clone();
        let runtime = &mut self.runtime;
        deno_core::scope!(scope, runtime);
        v8::tc_scope!(let tc, scope);
        let f = v8::Local::new(tc, &evaluate);
        let recv = v8::undefined(tc).into();
        let Some(result) = f.call(tc, recv, &[]) else {
            let msg = tc
                .exception()
                .map(|e| e.to_rust_string_lossy(tc))
                .unwrap_or_else(|| "unknown error".into());
            return Err(JsError::Eval(msg));
        };
        let array: v8::Local<v8::Array> = result
            .try_into()
            .map_err(|_| JsError::Eval("__evaluate did not return an array".into()))?;
        let mut out = Vec::with_capacity(array.length() as usize);
        for i in 0..array.length() {
            let v = array
                .get_index(tc, i)
                .ok_or_else(|| JsError::Eval(format!("__evaluate result missing index {i}")))?;
            out.push(v.boolean_value(tc));
        }
        Ok(out)
    }
}

impl Evaluator for Criteria {
    type Error = JsError;

    fn evaluate(&mut self, view: &PathView<'_>) -> Result<Vec<bool>, JsError> {
        let queries = {
            let state = self.runtime.op_state();
            let state = state.borrow();
            state.borrow::<CountTable>().queries.len()
        };
        let counts = (0..view.checkpoints())
            .map(|c| (0..queries).map(|q| view.count(c, q)).collect())
            .collect();
        self.set_counts(counts);
        self.call_evaluate()
    }
}

fn global_function(
    runtime: &mut JsRuntime,
    name: &str,
) -> Result<v8::Global<v8::Function>, JsError> {
    deno_core::scope!(scope, runtime);
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, name)
        .ok_or_else(|| JsError::Load(format!("could not intern {name}")))?;
    let value = global
        .get(scope, key.into())
        .ok_or_else(|| JsError::Load(format!("{name} is not defined")))?;
    let f: v8::Local<v8::Function> = value
        .try_into()
        .map_err(|_| JsError::Load(format!("{name} is not a function")))?;
    Ok(v8::Global::new(scope, f))
}

fn read_meta(runtime: &mut JsRuntime) -> Result<Vec<Criterion>, JsError> {
    let value = runtime
        .execute_script("[pe:meta]", "JSON.stringify(globalThis.__meta())")
        .map_err(|e| JsError::Load(e.to_string()))?;
    let json = {
        deno_core::scope!(scope, runtime);
        v8::Local::new(scope, value).to_rust_string_lossy(scope)
    };
    #[derive(serde::Deserialize)]
    struct Meta {
        name: String,
        #[serde(rename = "atLeast")]
        at_least: Option<f64>,
    }
    let metas: Vec<Meta> = serde_json::from_str(&json).map_err(|e| JsError::Load(e.to_string()))?;
    Ok(metas
        .into_iter()
        .map(|m| Criterion {
            name: m.name,
            at_least: m.at_least,
        })
        .collect())
}

/// Run criteria, discovering which queries they reference as they ask.
///
/// Criteria name their queries inline (`t(1).count('t:land')`), and short-circuit
/// operators mean a single probe run need not reach every one of them. So rather
/// than demand queries be declared up front, this asks the criteria to run, and
/// whenever JavaScript reaches for a query the current grouping does not cover,
/// the whole run is discarded and restarted with a grouping that does. That
/// terminates: the set of queries in a file is finite and only ever grows.
///
/// `build` turns the currently-known query list into a grouping, which is where
/// the caller applies card data.
pub fn run_with_discovery<E>(
    criteria: &mut Criteria,
    gaps: &[u32],
    mut build: impl FnMut(&[String]) -> Result<pe_criteria::Grouping, E>,
) -> Result<Vec<pe_stats::Probability>, DiscoveryError<E>> {
    let mut queries: Vec<String> = Vec::new();

    // A probe pass with everything zero finds most queries in one go; the loop
    // below is what makes it correct when branching hides some.
    criteria.set_queries(queries.clone());
    criteria.probe().map_err(DiscoveryError::Js)?;
    queries.append(&mut criteria.take_discovered());

    for _ in 0..MAX_DISCOVERY_ROUNDS {
        let grouping = build(&queries).map_err(DiscoveryError::Build)?;
        criteria.set_queries(queries.clone());

        let n = criteria.criteria().len();
        let result = pe_criteria::run(&grouping, gaps, n, criteria);

        let newly_found = criteria.take_discovered();
        if !newly_found.is_empty() {
            queries.extend(newly_found);
            continue;
        }
        return result.map_err(DiscoveryError::Run);
    }
    Err(DiscoveryError::DidNotSettle)
}

/// Each round strictly grows the query set, so this only trips on something
/// pathological like a criterion building query strings at random.
const MAX_DISCOVERY_ROUNDS: usize = 16;

#[derive(Debug, Error)]
pub enum DiscoveryError<E> {
    #[error(transparent)]
    Js(JsError),
    #[error("building the card grouping: {0}")]
    Build(E),
    #[error(transparent)]
    Run(pe_criteria::RunError<JsError>),
    #[error("criteria kept naming new queries after {MAX_DISCOVERY_ROUNDS} rounds")]
    DidNotSettle,
}

impl Criteria {
    /// The deepest turn any criterion has asked about so far.
    pub fn max_checkpoint(&self) -> u32 {
        let state = self.runtime.op_state();
        let state = state.borrow();
        state.borrow::<CountTable>().max_checkpoint
    }
}

impl Criteria {
    /// The queries the criteria settled on, once discovery has finished.
    pub fn queries(&self) -> Vec<String> {
        let state = self.runtime.op_state();
        let state = state.borrow();
        state.borrow::<CountTable>().queries.clone()
    }
}
