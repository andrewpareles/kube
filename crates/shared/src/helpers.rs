// Waiting for the process to be asked to stop is something only the native binaries do.
#[cfg(not(target_arch = "wasm32"))]
pub mod await_shutdown_signal;
pub mod hashmap_heap;
pub mod hashmap_queue;
pub mod hashmap_with_reverse;
pub mod iterator_round_robin;
pub mod multi_thread_hashmap;
pub mod sorted_vec_deque;
