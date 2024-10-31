(module
 (import "" "memory" (memory 0))
 (global $g0 (mut i64) (i64.const 0))
 (global $g1 (mut i64) (i64.const 0))
 (func (export "assume.const.memory") (param i32) (result i32)
       local.get 0)
 (func (export "assume.const.memory.transitive") (param i32) (result i32)
       local.get 0)
 (func (export "push.context") (param i32))
 (func (export "pop.context"))
 (func (export "update.context") (param i32))
 (func (export "read.reg") (param i64 i32) (result i64)
       local.get 1
       i64.load)
 (func (export "write.reg") (param i64 i32 i64)
       local.get 1
       local.get 2
       i64.store)
 (func (export "trace.line") (param i32))
 (func (export "abort.specialization") (param i32 i32))
 (func (export "assert.const32") (param i32 i32))
 (func (export "assert.const.memory") (param i32 i32))
 (func (export "specialize.value") (param i32 i32 i32) (result i32)
 local.get 0)
 (func (export "print") (param i32 i32 i32))
 (func (export "read.specialization.global") (param i32) (result i64) unreachable)
 (func (export "push.stack") (param i32 i64)
       local.get 0
       local.get 1
       i64.store)
 (func (export "sync.stack"))
 (func (export "read.stack") (param i32 i32) (result i64)
       local.get 1
       i64.load)
 (func (export "write.stack") (param i32 i32 i64)
       local.get 1
       local.get 2
       i64.store)
 (func (export "pop.stack") (param i32) (result i64)
       local.get 0
       i64.load)
 (func (export "read.local") (param i32 i32) (result i64)
       local.get 1
       i64.load)
 (func (export "write.local") (param i32 i32 i64)
       local.get 1
       local.get 2
       i64.store)
 (func (export "read.global.0") (result i64)
       global.get $g0)
 (func (export "write.global.0") (param i64)
       local.get 0
       global.set $g0)
 (func (export "read.global.1") (result i64) global.get $g1)
 (func (export "write.global.1") (param i64)
       local.get 0
       global.set $g1))
