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
use facet::Facet;
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
fn op_pe_count(state: &mut OpState, #[smi] checkpoint: u32, #[string] query: &str) -> u32 {
    let table = state.borrow_mut::<CountTable>();
    table.max_checkpoint = table.max_checkpoint.max(checkpoint);
    match table.queries.iter().position(|q| q == query) {
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
            if !table.discovered.iter().any(|q| q == query) {
                table.discovered.push(query.to_string());
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
    // Declaration order is drop order, and there are two constraints: a global
    // handle must be reset before the isolate holding it is disposed, and the
    // V8 runtime must go before the tokio runtime it was created inside.
    evaluate: v8::Global<v8::Function>,
    runtime: JsRuntime,
    /// V8 posts delayed tasks (garbage collection, mostly) and refuses to do so
    /// outside a tokio context. It only bites once enough calls accumulate, so
    /// without this the exact engine works and a long sampled run dies partway.
    _tokio: tokio::runtime::Runtime,
    criteria: Vec<Criterion>,
}

impl Criteria {
    /// Load a criteria file.
    pub fn load(source: String) -> Result<Self, JsError> {
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| JsError::Load(e.to_string()))?;
        let guard = tokio.enter();
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

        drop(guard);
        Ok(Criteria {
            evaluate,
            runtime,
            _tokio: tokio,
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

    /// Run every criterion once with all counts zero, purely to see which
    /// queries and turns they reach for. Whatever it answers is discarded.
    pub fn probe(&mut self) -> Result<(), JsError> {
        self.set_counts(Vec::new());
        self.call_evaluate().map(|_| ())
    }

    fn call_evaluate(&mut self) -> Result<Vec<bool>, JsError> {
        let _guard = self._tokio.enter();
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
    #[derive(Facet)]
    struct Meta {
        name: String,
        #[facet(rename = "atLeast")]
        at_least: Option<f64>,
    }
    let metas: Vec<Meta> = facet_json::from_str(&json).map_err(|e| JsError::Load(e.to_string()))?;
    Ok(metas
        .into_iter()
        .map(|m| Criterion {
            name: m.name,
            at_least: m.at_least,
        })
        .collect())
}

/// Each round strictly grows the query set and the turn horizon, so this only
/// trips on something pathological like a criterion building query strings at
/// random or indexing turns by what it drew.
const MAX_DISCOVERY_ROUNDS: usize = 16;

#[derive(Debug, Error)]
pub enum DiscoveryError<E, R> {
    #[error(transparent)]
    Js(JsError),
    #[error("building the card grouping: {0}")]
    Build(E),
    #[error(transparent)]
    Run(R),
    #[error("criteria kept naming new queries or turns after {MAX_DISCOVERY_ROUNDS} rounds")]
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

/// The discovery loop, over any runner.
///
/// Exact enumeration and sampling both need the same "run, notice what the
/// criteria reached for, start over" dance. Having one copy of it means the two
/// engines cannot drift apart in how they resolve a criteria file — only in how
/// they compute the answer.
///
/// Two things are discovered, and both for the same reason: criteria name their
/// queries and their turns inline, and short-circuiting operators mean no single
/// pass has to reach all of them. `a && b` with every count at zero never
/// evaluates `b`, so neither `b`'s query nor `b`'s turn is visible yet — and a
/// turn the run does not model answers 0 for every composition, which is the
/// silent, confident 0% this tool exists to prevent. So whenever a run reaches
/// past what it was set up for, the whole run is discarded and repeated against
/// a grouping and a horizon that cover it.
///
/// That terminates because both sets only ever grow, and a criteria file names
/// finitely many of each.
///
/// `gaps_for` turns a turn horizon into cards drawn between checkpoints, which
/// is where the caller applies the rules of the game.
pub fn with_discovery<E, R, T>(
    criteria: &mut Criteria,
    mut build: impl FnMut(&[String]) -> Result<pe_criteria::Grouping, E>,
    gaps_for: impl Fn(u32) -> Vec<u32>,
    run: impl Fn(&pe_criteria::Grouping, &[u32], &mut Criteria) -> Result<T, R>,
) -> Result<T, DiscoveryError<E, R>> {
    let mut queries: Vec<String> = Vec::new();

    criteria.set_queries(queries.clone());
    criteria.probe().map_err(DiscoveryError::Js)?;
    queries.append(&mut criteria.take_discovered());
    let mut horizon = criteria.max_checkpoint();

    for _ in 0..MAX_DISCOVERY_ROUNDS {
        let grouping = build(&queries).map_err(DiscoveryError::Build)?;
        criteria.set_queries(queries.clone());
        let gaps = gaps_for(horizon);

        let result = run(&grouping, &gaps, criteria);

        let newly_found = criteria.take_discovered();
        let reached_deeper = criteria.max_checkpoint() > horizon;
        if !newly_found.is_empty() || reached_deeper {
            queries.extend(newly_found);
            horizon = criteria.max_checkpoint();
            continue;
        }
        return result.map_err(DiscoveryError::Run);
    }
    Err(DiscoveryError::DidNotSettle)
}
