// use core::mem::MaybeUninit;
// use embedded_alloc::LlffHeap;

// #[global_allocator]
// static HEAP: LlffHeap = LlffHeap::empty();

// const HEAP_SIZE: usize = 1024 * 4;
// static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];

// pub fn init() {
//     unsafe { HEAP.init(HEAP_MEM.as_ptr() as usize, HEAP_SIZE) }
// }
