;; Minimal v1-ABI plugin used by the runtime tests.
;;
;; - Exports `memory`, `alloc`, and `handle`.
;; - Ignores the request and returns a hard-coded JSON body
;;   `{"status":200,"body":"ok"}` (26 bytes long, written at offset 0).
;; - `alloc` is a trivial bump allocator starting at offset 1024 so it
;;   never collides with the hard-coded response data.
(module
  (memory (export "memory") 1)
  (global $heap (mut i32) (i32.const 1024))

  (data (i32.const 0) "{\"status\":200,\"body\":\"ok\"}")

  (func (export "alloc") (param $size i32) (result i32)
    (local $p i32)
    (local.set $p (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $size)))
    (local.get $p)
  )

  (func (export "handle")
    (param $mp i32) (param $ml i32)
    (param $pp i32) (param $pl i32)
    (param $bp i32) (param $bl i32)
    (result i64)
    ;; (ptr=0 << 32) | len=26
    (i64.const 26)
  )
)
