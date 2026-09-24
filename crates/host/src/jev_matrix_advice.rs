//! Pure Matrix ranking wire contract. Preparing or parsing does not dispatch Jev,
//! select an effective choice, or confer release authority.

mod wire;

pub use wire::{
    MatrixRankingBinding, ParsedMatrixRankingResponse, PreparedMatrixRankingRequest,
    parse_response, prepare_request,
};
