use bincode::{
    DefaultOptions, Options,
    config::{VarintEncoding, WithOtherIntEncoding},
};

pub type BincodeConfig = WithOtherIntEncoding<DefaultOptions, VarintEncoding>;

// essentially free to call
pub fn get_bincode_config() -> BincodeConfig {
    DefaultOptions::new().with_varint_encoding()
}
