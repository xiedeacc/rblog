;; Hello-world example plugin written in raw WebAssembly text format
;; so it requires zero toolchain to play with. Compile it with
;; `wat2wasm plugin.wat -o plugin.wasm` if you want a `.wasm` binary
;; — the runtime accepts both because the `wat` feature of wasmtime is
;; enabled at build time.
;;
;; This plugin:
;; - Exports the v1 ABI (`memory`, `alloc`, `handle`).
;; - Imports `host_log` to log a single line on every request.
;; - Always returns the same JSON response: status 200, content type
;;   `text/plain; charset=utf-8`, body `Hello, world!`.
;;
;; A real plugin would inspect the request method/path/body buffers
;; via the parameters passed to `handle` and shape a dynamic response.
(module
  ;; Imports MUST come first in a WASM module (section ordering rule).
  ;; This one is gated by the `log` capability in plugin.toml.
  (import "env" "host_log" (func $host_log (param i32 i32 i32)))

  ;; The host expects to find an exported memory named "memory".
  (memory (export "memory") 1)

  ;; A tiny bump allocator: every `alloc(size)` advances the heap pointer
  ;; past `size` bytes and returns the old value.
  (global $heap (mut i32) (i32.const 1024))

  ;; Response payload: kept at offset 0 in linear memory.
  ;;   `{"status":200,"content_type":"text/plain; charset=utf-8",`
  ;;   `"body":"Hello, world!"}` — 80 characters.
  (data
    (i32.const 0)
    "{\"status\":200,\"content_type\":\"text/plain; charset=utf-8\",\"body\":\"Hello, world!\"}"
  )

  ;; Log message: also kept inline at offset 200.
  (data (i32.const 200) "hello-world plugin invoked")

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
    ;; host_log(level=2 /* info */, msg_ptr=200, msg_len=26)
    (call $host_log (i32.const 2) (i32.const 200) (i32.const 26))
    ;; return packed (ptr=0 << 32) | len=80
    (i64.const 80)
  )
)
