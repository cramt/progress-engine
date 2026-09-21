//! The job protocol's wire format.
//!
//! Engine results are MessagePack, not JSON (FINDINGS.md §7). They are written
//! into the wasm filesystem, so only the sandbox can pick them up - but it does
//! not have to understand them: the glue hands the bytes straight back out
//! through `op_probe_decode_job` and gets JSON in return.
//!
//! Parsing here rather than in JavaScript is what makes the shape below the
//! only shape a result can have. A blob that is not one of these is a decode
//! error the host can name, instead of a plausible-looking object assembled by
//! whatever the tag bytes happened to say.

use anyhow::{anyhow, Result};
use facet::Facet;
use facet_value::Value;

/// One `main.output` or `worker_N.output` blob.
#[derive(Facet, Debug)]
pub struct JobMessage {
    /// The requestId the export returned, and what `_process_next` acks.
    pub id: u64,
    pub status: JobStatus,
    /// 0-100.
    #[facet(default)]
    pub progress: u32,
    /// The failure text on `error`, the current step on `running`. Absent on
    /// plenty of results, hence the default.
    #[facet(default)]
    pub message: String,
    /// Whatever the export returns - absent, null, an array of rows, or an
    /// object. One export's payload is not another's, so this is the one part
    /// of a result that stays dynamic.
    #[facet(default)]
    pub data: Value,
}

/// `running` is a progress update on the same channel as the result, and must
/// not consume the pending request - see `JobQueue` in `js/engine.js`.
#[derive(Facet, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[facet(rename_all = "lowercase")]
pub enum JobStatus {
    Success,
    Error,
    Running,
}

/// Decode one blob into the JSON the glue reads.
pub fn decode_to_json(bytes: &[u8]) -> Result<String> {
    let message: JobMessage =
        facet_msgpack::from_slice(bytes).map_err(|e| anyhow!("decoding a job result: {e}"))?;
    facet_json::to_string(&message).map_err(|e| anyhow!("re-encoding a job result: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    /// Captured off a real run (`PROBE_LOG=1`, one blob per line of the drain
    /// loop), which is the only authority on what the engine actually writes:
    /// every one of the 55 blobs a boot-query-recognise run produced decodes to
    /// exactly what `js/msgpack.js` used to produce for it.
    const REAL: &[(&str, &str)] = &[
        // a progress update, with no data at all
        (
            "84a2696403a76d657373616765aa46696e616c697a696e67a870726f677265737364a673746174\
             7573a772756e6e696e67",
            r#"{"id":3,"status":"running","progress":100,"message":"Finalizing","data":null}"#,
        ),
        // _rec_best_detection: the quad locate() reshapes into four points
        (
            "84a46461746182a7636f726e65727398cd01cdccefcd0327cce7cd0332cd02e2cd01cdcd02d8ab6d\
             6f64656c4c6f61646564c3a269640aa870726f677265737364a6737461747573a773756363657373",
            r#"{"id":10,"status":"success","progress":100,"message":"","data":{"corners":[461,239,807,231,818,738,461,728],"modelLoaded":true}}"#,
        ),
        // _installation_status
        (
            "84a46461746183a9686173557064617465c3a9686173557365724462c2a9696e7374616c6c6564c2\
             a2696402a870726f677265737364a6737461747573a773756363657373",
            r#"{"id":2,"status":"success","progress":100,"message":"","data":{"hasUpdate":true,"hasUserDb":false,"installed":false}}"#,
        ),
        // _sql_query: rows arrive as arrays of strings, whatever the column type
        (
            "84a4646174619197ab426c61636b204c6f747573b1556e6c696d697465642045646974696f6ea332\
             3333a152b04368726973746f706865722052757368d92434613265343238632d646432352d343834\
             632d626263382d326436636531306566343263aa313939332d31322d3031a2696409a870726f6772\
             65737364a6737461747573a773756363657373",
            r#"{"id":9,"status":"success","progress":100,"message":"","data":[["Black Lotus","Unlimited Edition","233","R","Christopher Rush","4a2e428c-dd25-484c-bbc8-2d6ce10ef42c","1993-12-01"]]}"#,
        ),
        // hand-built: a healthy run never produces one, and the glue's error
        // path is only reachable through it
        (
            "85a464617461c0a2696407a76d657373616765b36e6f2073756368207461626c653a206e6f7065a8\
             70726f677265737300a6737461747573a56572726f72",
            r#"{"id":7,"status":"error","progress":0,"message":"no such table: nope","data":null}"#,
        ),
    ];

    #[test]
    fn real_job_results_decode_to_the_json_the_glue_expects() {
        for (bytes, want) in REAL {
            let bytes = hex(&bytes.replace(['\n', ' '], ""));
            assert_eq!(decode_to_json(&bytes).unwrap(), *want);
        }
    }

    /// The point of decoding against a declared shape: a blob that is not a job
    /// result fails loudly here rather than turning into an object with the
    /// wrong fields in it.
    #[test]
    fn a_blob_that_is_not_a_job_result_is_an_error() {
        let good = hex(&REAL[0].0.replace(['\n', ' '], ""));
        let truncated = decode_to_json(&good[..10]).unwrap_err().to_string();
        assert!(truncated.contains("end of input"), "{truncated}");

        // 0x82 fixmap(2): {"id": 1, "status": "queued"}
        let unknown_status = hex("82a2696401a6737461747573a6717565756564");
        let err = decode_to_json(&unknown_status).unwrap_err().to_string();
        assert!(err.contains("enum variant"), "{err}");
    }
}
