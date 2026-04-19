//! Compares Cranelift block JIT vs the opcode interpreter on the same bytecode: a numeric `for` loop
//! with frame slots (`$i`, `$sum`), same shape as `vm_chunk_block_jit_for_loop` in `src/jit.rs`.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use stryke::bytecode::{Chunk, Op};
use stryke::interpreter::Interpreter;
use stryke::vm::VM;

/// `for ($i=0; $i<limit; $i++) { $sum += $i }` — returns sum of `0..limit-1`.
fn block_jit_sum_chunk(limit: i64) -> Chunk {
    let mut c = Chunk::new();
    let ni = c.intern_name("i");
    let ns = c.intern_name("sum");
    c.emit(Op::LoadInt(0), 1);
    c.emit(Op::DeclareScalarSlot(0, ni), 1);
    c.emit(Op::LoadInt(0), 1);
    c.emit(Op::DeclareScalarSlot(1, ns), 1);
    c.emit(Op::GetScalarSlot(0), 1);
    c.emit(Op::LoadInt(limit), 1);
    c.emit(Op::NumLt, 1);
    c.emit(Op::JumpIfFalse(15), 1);
    c.emit(Op::GetScalarSlot(1), 1);
    c.emit(Op::GetScalarSlot(0), 1);
    c.emit(Op::Add, 1);
    c.emit(Op::SetScalarSlot(1), 1);
    c.emit(Op::PostIncSlot(0), 1);
    c.emit(Op::Pop, 1);
    c.emit(Op::Jump(4), 1);
    c.emit(Op::GetScalarSlot(1), 1);
    c.emit(Op::Halt, 1);
    c
}

fn jit_block_loop(c: &mut Criterion) {
    // Large enough to dominate JIT compile + syscall noise; sum = n*(n-1)/2 for i in 0..n-1.
    let limit = 200_000i64;
    let chunk = block_jit_sum_chunk(limit);

    let mut g = c.benchmark_group("block_loop_sum_slots");
    g.bench_function("jit_on", |b| {
        b.iter(|| {
            let mut interp = Interpreter::new();
            let mut vm = VM::new(&chunk, &mut interp);
            vm.set_jit_enabled(true);
            black_box(vm.execute().expect("vm").to_int())
        })
    });
    g.bench_function("jit_off", |b| {
        b.iter(|| {
            let mut interp = Interpreter::new();
            let mut vm = VM::new(&chunk, &mut interp);
            vm.set_jit_enabled(false);
            black_box(vm.execute().expect("vm").to_int())
        })
    });
    g.finish();
}

criterion_group!(benches, jit_block_loop);
criterion_main!(benches);
