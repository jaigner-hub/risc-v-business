use std::collections::HashMap;
use dynasmrt::{dynasm, DynasmApi, ExecutableBuffer, x64::Assembler};
use crate::cpu::Cpu;

/// Signature of every compiled basic block.
/// - `regs`: pointer to `cpu.regs[0]` — the 32-element u64 register file.
/// - `cpu`: opaque pointer passed through to memory callout helpers.
/// Returns: next guest PC, or `u64::MAX` for slow-path (trap / unhandled instruction).
pub type JitFn = unsafe extern "sysv64" fn(regs: *mut u64, cpu: *mut Cpu) -> u64;

pub struct JitCache {
    blocks: HashMap<u64, (ExecutableBuffer, JitFn)>,
}

impl JitCache {
    pub fn new() -> Self {
        Self { blocks: HashMap::new() }
    }

    /// Look up a compiled block for `pc`. Returns `None` if not yet compiled.
    pub fn get(&self, pc: u64) -> Option<JitFn> {
        self.blocks.get(&pc).map(|&(_, f)| f)
    }

    /// Flush the entire block cache (called on satp write and sfence.vma).
    pub fn invalidate(&mut self) {
        self.blocks.clear();
    }

    /// Compile the basic block starting at guest virtual address `pc`.
    /// No-op if the block is already cached or if instruction fetch fails.
    pub fn compile(&mut self, cpu: &mut Cpu, pc: u64) {
        if self.blocks.contains_key(&pc) { return; }
        // Implemented in Tasks 3–6.
        let _ = cpu;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{bus::Bus, cpu::Cpu};

    #[allow(dead_code)]
    fn make_cpu() -> Cpu {
        Cpu::new(Bus::new(64, 0x8000_0000), 0x8000_0000, false)
    }

    #[test]
    fn jit_cache_new_is_empty() {
        let jit = JitCache::new();
        assert!(jit.get(0x8000_0000).is_none());
    }

    #[test]
    fn jit_cache_invalidate_clears_all() {
        let mut jit = JitCache::new();
        // manually insert a dummy block to test invalidate
        let mut ops = Assembler::new().unwrap();
        let off = ops.offset();
        dynasm!(ops ; .arch x64 ; mov rax, QWORD 42i64 ; ret);
        let buf = ops.finalize().unwrap();
        let f: JitFn = unsafe { std::mem::transmute(buf.ptr(off)) };
        jit.blocks.insert(0x8000_0000, (buf, f));
        assert!(jit.get(0x8000_0000).is_some());
        jit.invalidate();
        assert!(jit.get(0x8000_0000).is_none());
    }
}
