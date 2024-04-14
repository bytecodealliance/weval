(module
 (func (export "assume.const.memory") (param i32) (result i32)
       local.get 0)
 (func (export "assume.const.memory.transitive") (param i32) (result i32)
       local.get 0)
 (func (export "push.context") (param i32))
 (func (export "pop.context"))
 (func (export "update.context") (param i32))
 (func (export "read.reg") (param i64) (result i64)
       i64.const 0)
 (func (export "write.reg") (param i64 i64))
 (func (export "trace.line") (param i32))
 (func (export "abort.specialization") (param i32 i32))
 (func (export "assert.const32") (param i32 i32))
 (func (export "assert.const.memory") (param i32 i32))
 (func (export "specialize.value") (param i32 i32 i32) (result i32)
 local.get 0)
 (func (export "print") (param i32 i32 i32))
 (func (export "global.get") (param i64) (result i64) i64.const 0)
 (func (export "global.set") (param i64 i64)))
