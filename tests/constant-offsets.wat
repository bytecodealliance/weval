(module
  (memory 1)
  (data (i32.const 0) "\11\00\00\00\1d\00\00\00")
  (func (export "compute") (param $base i32) (param $count i32) (param $branch i32) (result i32)
    (local $pointer i32)
    local.get $base
    i32.const 16
    i32.sub
    local.set $pointer
    block $done
      local.get $count
      i32.eqz
      br_if $done
      ;; This extra diamond exposes the missed update across the later join.
      local.get $branch
      if (result i32)
        i32.const 3
      else
        i32.const 5
      end
      drop
      loop $decrement
        local.get $pointer
        i32.const 8
        i32.sub
        local.set $pointer
        local.get $count
        i32.const 1
        i32.sub
        local.tee $count
        br_if $decrement
      end
    end
    local.get $branch
    if (result i32)
      i32.const 0
      i32.load
    else
      i32.const 4
      i32.load
    end
    local.get $pointer
    i32.const 8
    i32.sub
    i32.add))
