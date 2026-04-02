pub mod bincode_codec;
pub mod borsh_codec;
pub mod error;

pub use bincode_codec::{decode_bincode, encode_bincode};
pub use borsh_codec::{decode_borsh, encode_borsh};
pub use error::{CodecError, Result};
