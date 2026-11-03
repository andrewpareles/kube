// this can unwrap and panic because it always gets called initially
pub fn initialize_env() {
    // values used by every crate
    dotenvy::from_read(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packages/website/.env")).as_bytes()).unwrap();
}
