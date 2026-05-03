use std::collections::HashMap;
use dynasmrt::ExecutableBuffer;
use crate::cpu::Cpu;

/// Signature of every compiled basic block.
/// - `regs`: pointer to `cpu.regs[0]` — the 32-element u64 register file.
/// - `cpu`: opaque pointer passed through to memory callout helpers.
/// Returns: next guest PC, or `u64::MAX` for slow-path (trap / unhandled instruction).
pub type JitFn = unsafe extern "sysv64" fn(regs: *mut u64, cpu: *mut Cpu) -> u64;

pub struct JitCache {
    blocks: HashMap<u64, (ExecutableBuffer, JitFn)>,
}

/// Load 1 byte (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load8(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    cpu.bus.load(addr, 1).unwrap_or(u64::MAX)
}

/// Load 2 bytes (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load16(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    cpu.bus.load(addr, 2).unwrap_or(u64::MAX)
}

/// Load 4 bytes (zero-extended to u64). Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load32(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    cpu.bus.load(addr, 4).unwrap_or(u64::MAX)
}

/// Load 8 bytes. Returns u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_load64(cpu: *mut Cpu, addr: u64) -> u64 {
    let cpu = &mut *cpu;
    cpu.bus.load(addr, 8).unwrap_or(u64::MAX)
}

/// Store 1 byte. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store8(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(addr, 1, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 2 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store16(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(addr, 2, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 4 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store32(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(addr, 4, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

/// Store 8 bytes. Returns 0 on success, u64::MAX on fault.
///
/// # Safety
///
/// `cpu` must be a non-null, properly aligned pointer to a live `Cpu` with no
/// other mutable references to `*cpu` at the point of the call.
#[no_mangle]
pub unsafe extern "sysv64" fn jit_store64(cpu: *mut Cpu, addr: u64, val: u64) -> u64 {
    let cpu = &mut *cpu;
    match cpu.bus.store(addr, 8, val) {
        Ok(_)  => 0,
        Err(_) => u64::MAX,
    }
}

type JitLoadFn  = unsafe extern "sysv64" fn(*mut Cpu, u64) -> u64;
type JitStoreFn = unsafe extern "sysv64" fn(*mut Cpu, u64, u64) -> u64;

/// Forces the linker to retain the eight memory-callout helpers above through LTO.
/// JIT-generated machine code calls them by absolute address, so the compiler
/// cannot otherwise see them as live. `#[used]` is only valid on statics, hence
/// these arrays of function-pointer constants.
#[used]
static JIT_LOAD_CALLOUTS: [JitLoadFn; 4] =
    [jit_load8, jit_load16, jit_load32, jit_load64];
#[used]
static JIT_STORE_CALLOUTS: [JitStoreFn; 4] =
    [jit_store8, jit_store16, jit_store32, jit_store64];

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
    use dynasmrt::{dynasm, DynasmApi, x64::Assembler};

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

    #[test]
    fn callout_store_then_load_roundtrip() {
        let mut cpu = make_cpu();
        cpu.regs[1] = 0xDEAD_BEEF_0000_0001;
        let addr = 0x8000_0010u64;

        let store_result = unsafe { jit_store64(&mut cpu as *mut Cpu, addr, cpu.regs[1]) };
        assert_eq!(store_result, 0, "store64 should return 0 on success");

        let loaded = unsafe { jit_load64(&mut cpu as *mut Cpu, addr) };
        assert_eq!(loaded, 0xDEAD_BEEF_0000_0001);
    }

    #[test]
    fn callout_load_fault_returns_sentinel() {
        let mut cpu = make_cpu();
        // Address 0x0 is outside RAM — should return u64::MAX
        let result = unsafe { jit_load64(&mut cpu as *mut Cpu, 0x0000_0000) };
        assert_eq!(result, u64::MAX);
    }

    #[test]
    fn callout_store_fault_returns_sentinel() {
        let mut cpu = make_cpu();
        let result = unsafe { jit_store64(&mut cpu as *mut Cpu, 0x0000_0000, 42) };
        assert_eq!(result, u64::MAX);
    }
}
