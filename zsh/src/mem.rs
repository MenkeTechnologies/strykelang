//! Memory management for zshrs
//!
//! Port from zsh/Src/mem.c
//!
//! In Rust, we don't need the complex heap management that zsh uses in C.
//! Instead, we provide a simpler arena-style allocator abstraction that
//! can be used for temporary allocations that all get freed at once.

use std::cell::RefCell;

/// A memory arena for temporary allocations
/// 
/// This provides push/pop semantics similar to zsh's heap management,
/// but uses Rust's standard memory management under the hood.
pub struct HeapArena {
    /// Stack of arena generations
    generations: Vec<Generation>,
}

struct Generation {
    /// Strings allocated in this generation
    strings: Vec<String>,
    /// Byte buffers allocated in this generation
    buffers: Vec<Vec<u8>>,
}

impl Default for HeapArena {
    fn default() -> Self {
        Self::new()
    }
}

impl HeapArena {
    pub fn new() -> Self {
        HeapArena {
            generations: vec![Generation {
                strings: Vec::new(),
                buffers: Vec::new(),
            }],
        }
    }

    /// Push a new heap state (like zsh's pushheap)
    pub fn push(&mut self) {
        self.generations.push(Generation {
            strings: Vec::new(),
            buffers: Vec::new(),
        });
    }

    /// Pop and free all allocations since the last push (like zsh's popheap)
    pub fn pop(&mut self) {
        if self.generations.len() > 1 {
            self.generations.pop();
        }
    }

    /// Free allocations in current generation but keep generation marker (like zsh's freeheap)
    pub fn free_current(&mut self) {
        if let Some(gen) = self.generations.last_mut() {
            gen.strings.clear();
            gen.buffers.clear();
        }
    }

    /// Allocate a string in the current generation
    pub fn alloc_string(&mut self, s: String) -> &str {
        if let Some(gen) = self.generations.last_mut() {
            gen.strings.push(s);
            gen.strings.last().map(|s| s.as_str()).unwrap()
        } else {
            panic!("No generation available")
        }
    }

    /// Allocate bytes in the current generation
    pub fn alloc_bytes(&mut self, bytes: Vec<u8>) -> &[u8] {
        if let Some(gen) = self.generations.last_mut() {
            gen.buffers.push(bytes);
            gen.buffers.last().map(|b| b.as_slice()).unwrap()
        } else {
            panic!("No generation available")
        }
    }

    /// Get current stack depth
    pub fn depth(&self) -> usize {
        self.generations.len()
    }
}

thread_local! {
    static HEAP: RefCell<HeapArena> = RefCell::new(HeapArena::new());
}

/// Push heap state
pub fn pushheap() {
    HEAP.with(|h| h.borrow_mut().push());
}

/// Pop heap state and free allocations
pub fn popheap() {
    HEAP.with(|h| h.borrow_mut().pop());
}

/// Free current heap allocations but keep state
pub fn freeheap() {
    HEAP.with(|h| h.borrow_mut().free_current());
}

/// Allocate memory (in Rust, this just uses the normal allocator)
pub fn zalloc<T: Default>() -> Box<T> {
    Box::default()
}

/// Allocate zeroed memory
pub fn zshcalloc<T: Default>() -> Box<T> {
    Box::default()
}

/// Reallocate memory (Rust handles this automatically with Vec)
pub fn zrealloc<T>(v: &mut Vec<T>, new_size: usize) 
where
    T: Default + Clone,
{
    v.resize(new_size, T::default());
}

/// Free memory (no-op in Rust, drop handles it)
pub fn zfree<T>(_ptr: Box<T>) {
    // Drop happens automatically
}

/// Free a string (no-op in Rust)
pub fn zsfree(_s: String) {
    // Drop happens automatically
}

/// Duplicate a string
pub fn dupstring(s: &str) -> String {
    s.to_string()
}

/// Duplicate a string with length
pub fn dupstring_wlen(s: &str, len: usize) -> String {
    s.chars().take(len).collect()
}

/// Create a heap-allocated string (in Rust, just creates a String)
pub fn zhalloc_string(s: &str) -> String {
    s.to_string()
}

/// Check if a pointer is within a memory pool
/// In Rust, we don't need this - just returns true for any valid reference
pub fn zheapptr<T>(_ptr: &T) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_push_pop() {
        let mut arena = HeapArena::new();
        assert_eq!(arena.depth(), 1);
        
        arena.push();
        assert_eq!(arena.depth(), 2);
        
        arena.alloc_string("test".to_string());
        
        arena.pop();
        assert_eq!(arena.depth(), 1);
    }

    #[test]
    fn test_heap_free_current() {
        let mut arena = HeapArena::new();
        
        arena.alloc_string("test1".to_string());
        arena.alloc_bytes(vec![1, 2, 3]);
        
        arena.free_current();
        // Arena still at depth 1
        assert_eq!(arena.depth(), 1);
    }

    #[test]
    fn test_nested_generations() {
        let mut arena = HeapArena::new();
        
        arena.alloc_string("level1".to_string());
        
        arena.push();
        arena.alloc_string("level2".to_string());
        
        arena.push();
        arena.alloc_string("level3".to_string());
        
        assert_eq!(arena.depth(), 3);
        
        arena.pop();
        assert_eq!(arena.depth(), 2);
        
        arena.pop();
        assert_eq!(arena.depth(), 1);
    }

    #[test]
    fn test_dupstring() {
        let s = dupstring("hello");
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_dupstring_wlen() {
        let s = dupstring_wlen("hello world", 5);
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_global_heap() {
        pushheap();
        pushheap();
        popheap();
        popheap();
        // Should not panic
    }
}
