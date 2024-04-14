#pragma once

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------------- */
/* partial-evaluation async requests and queues                              */
/* ------------------------------------------------------------------------- */

typedef void (*weval_func_t)();

typedef struct weval_req_t weval_req_t;
typedef struct weval_req_arg_t weval_req_arg_t;
typedef struct weval_lookup_entry_t weval_lookup_entry_t;
typedef struct weval_lookup_t weval_lookup_t;

struct weval_req_t {
  weval_req_t* next;
  weval_req_t* prev;
  /* A user-provided ID of the weval'd function, for stability of
   * collected request bodies across relinkings: */
  uint32_t func_id;
  weval_func_t func;
  uint8_t* argbuf;
  uint32_t arglen;
  weval_func_t* specialized;
};

typedef enum {
  weval_req_arg_i32 = 0,
  weval_req_arg_i64 = 1,
  weval_req_arg_f32 = 2,
  weval_req_arg_f64 = 3,
  weval_req_arg_buffer = 4,
  weval_req_arg_none = 255,
} weval_req_arg_type;

struct weval_req_arg_t {
  uint32_t specialize; /* is this argument specialized? */
  uint32_t
      ty; /* type of specialization value (`weval_req_arg_type` enum value). */
  /* The value to specialize on: */
  union {
    uint64_t raw;
    uint32_t i32;
    uint64_t i64;
    float f32;
    double f64;
    struct {
      /* A pointer to arbitrary memory with constant contents of the
       * given length; data follows. */
      uint32_t len;
      /* Size of buffer in data stream; next arg follows inline data. */
      uint32_t padded_len;
    } buffer;
  } u;
};

/* Lookup table created by weval for pre-inserted wevaled function bodies */
struct weval_lookup_t {
  weval_lookup_entry_t* entries;
  uint32_t nentries;
};

struct weval_lookup_entry_t {
  uint32_t func_id;
  const uint8_t* argbuf;
  uint32_t arglen;
  weval_func_t specialized;
};

extern weval_req_t* weval_req_pending_head;
extern bool weval_is_wevaled;
extern weval_lookup_t weval_lookup_table;

#define WEVAL_DEFINE_GLOBALS()                                          \
  weval_req_t* weval_req_pending_head;                                  \
  __attribute__((export_name("weval.pending.head"))) weval_req_t**      \
  __weval_pending_head() {                                              \
    return &weval_req_pending_head;                                     \
  }                                                                     \
                                                                        \
  bool weval_is_wevaled;                                                \
  __attribute__((export_name("weval.is.wevaled"))) bool*                \
  __weval_is_wevaled() {                                                \
    return &weval_is_wevaled;                                           \
  }                                                                     \
                                                                        \
  weval_lookup_t weval_lookup_table = {.entries = NULL, .nentries = 0}; \
  __attribute__((export_name("weval.lookup.table"))) weval_lookup_t*    \
  __weval_lookup_table() {                                              \
    return &weval_lookup_table;                                         \
  }

#define WEVAL_DEFINE_TARGET(index, func)             \
  __attribute__((export_name("weval.func." #index))) \
      weval_func_t __weval_func_##index() {          \
    return (weval_func_t) & (func);                  \
  }

/* Compare entry to req; return -1 for less than, 1 for greater than,
 * 0 for equal. */
static inline int __weval_binsearch_cmp(weval_req_t* req, uint32_t idx) {
  weval_lookup_entry_t* entry = &weval_lookup_table.entries[idx];
  if (entry->func_id < req->func_id) {
    return -1;
  } else if (entry->func_id > req->func_id) {
    return 1;
  } else {
    uint32_t min_len =
        req->arglen < entry->arglen ? req->arglen : entry->arglen;
    int cmp = memcmp(entry->argbuf, req->argbuf, min_len);
    if (cmp != 0) {
      return cmp;
    } else if (entry->arglen < req->arglen) {
      return -1;
    } else if (entry->arglen > req->arglen) {
      return 1;
    } else {
      return 0;
    }
  }
}

static inline weval_lookup_entry_t* __weval_find(weval_req_t* req) {
  if (weval_lookup_table.nentries == 0) {
    return NULL;
  }

  uint32_t lo = 0;
  uint32_t hi = weval_lookup_table.nentries;

  while (hi > lo) {
    uint32_t mid = lo + (hi - lo) / 2;
    int cmp = __weval_binsearch_cmp(req, mid);
    if (cmp == 0) {
      return &weval_lookup_table.entries[mid];
    } else if (cmp < 0) {
      lo = mid + 1;
    } else if (cmp > 0) {
      hi = mid;
    }
  }

  return NULL;
}

static inline void weval_request(weval_req_t* req) {
  if (weval_is_wevaled) {
    weval_lookup_entry_t* entry = __weval_find(req);
    if (entry) {
      *req->specialized = entry->specialized;
    }
  } else {
    req->next = weval_req_pending_head;
    req->prev = NULL;
    if (weval_req_pending_head) {
      weval_req_pending_head->prev = req;
    }
    weval_req_pending_head = req;
  }
}

static inline void weval_free(weval_req_t* req) {
  if (req->prev) {
    req->prev->next = req->next;
  } else if (weval_req_pending_head == req) {
    weval_req_pending_head = req->next;
  }
  if (req->next) {
    req->next->prev = req->prev;
  }
  if (req->argbuf) {
    free(req->argbuf);
  }
  free(req);
}

/* ------------------------------------------------------------------------- */
/* intrinsics                                                                */
/* ------------------------------------------------------------------------- */

#ifdef __cplusplus
extern "C" {
#endif

#define WEVAL_WASM_IMPORT(name) \
  __attribute__((__import_module__("weval"), __import_name__(name)))

/* Core intrinsics for interpreter loops: contexts, registers, value
 * specialization */
    
void weval_push_context(uint32_t pc) WEVAL_WASM_IMPORT("push.context");
void weval_pop_context() WEVAL_WASM_IMPORT("pop.context");
void weval_update_context(uint32_t pc) WEVAL_WASM_IMPORT("update.context");
uint64_t weval_read_reg(uint64_t idx) WEVAL_WASM_IMPORT("read.reg");
void weval_write_reg(uint64_t idx, uint64_t value)
    WEVAL_WASM_IMPORT("write.reg");
uint32_t weval_specialize_value(uint32_t value, uint32_t lo, uint32_t hi)
    WEVAL_WASM_IMPORT("specialize.value");
uint64_t weval_read_global(uint64_t idx) WEVAL_WASM_IMPORT("read.global");
void weval_write_global(uint64_t idx, uint64_t value)
    WEVAL_WASM_IMPORT("write.global");

/* Debugging and stats intrinsics */
    
void weval_trace_line(uint32_t line_number) WEVAL_WASM_IMPORT("trace.line");
void weval_abort_specialization(uint32_t line_number, uint32_t fatal)
    WEVAL_WASM_IMPORT("abort.specialization");
void weval_assert_const32(uint32_t value, uint32_t line_no)
    WEVAL_WASM_IMPORT("assert.const32");
void weval_print(const char* message, uint32_t line, uint32_t val)
    WEVAL_WASM_IMPORT("print");
void weval_context_bucket(uint32_t bucket) WEVAL_WASM_IMPORT("context.bucket");

#undef WEVAL_WASM_IMPORT

#ifdef __cplusplus
}  // extern "C"
#endif

#ifdef __cplusplus
namespace weval {
static inline void push_context(uint32_t pc) { weval_push_context(pc); }
static inline void pop_context() { weval_pop_context(); }
static inline void update_context(uint32_t pc) { weval_update_context(pc); }
}  // namespace weval
#endif  // __cplusplus

/* ------------------------------------------------------------------------- */
/* C++ type-safe wrapper for partial evaluation of functions                 */
/* ------------------------------------------------------------------------- */

#ifdef __cplusplus
namespace weval {

struct ArgWriter {
  static const size_t MAX = 1024 * 1024;

  uint8_t* buffer;
  size_t len;
  size_t cap;

  ArgWriter() : buffer(nullptr), len(0), cap(0) {}

  uint8_t* alloc(size_t bytes) {
    if (bytes + len > MAX) {
      return nullptr;
    }
    if (bytes + len > cap) {
      size_t desired_cap = (cap == 0) ? 1024 : cap;
      while (desired_cap < (len + bytes)) {
        desired_cap *= 2;
      }
      buffer = reinterpret_cast<uint8_t*>(realloc(buffer, desired_cap));
      if (!buffer) {
        return nullptr;
      }
      cap = desired_cap;
    }
    uint8_t* ret = buffer + len;
    len += bytes;
    return ret;
  }

  template <typename T>
  bool write(T t) {
    uint8_t* mem = alloc(sizeof(T));
    if (!mem) {
      return false;
    }
    memcpy(mem, reinterpret_cast<uint8_t*>(&t), sizeof(T));
    return true;
  }

  uint8_t* take() {
    uint8_t* ret = buffer;
    buffer = nullptr;
    len = 0;
    cap = 0;
    return ret;
  }
};

template <typename T>
struct ArgSpec {};

template <typename T>
struct RuntimeArg : ArgSpec<T> {};

template <typename T>
RuntimeArg<T> Runtime() {
  return RuntimeArg<T>{};
}

template <typename T>
struct Specialize : ArgSpec<T> {
  T value;
  explicit Specialize(T value_) : value(value_) {}
};

template <typename T>
struct SpecializeMemory : ArgSpec<T> {
  T ptr;
  uint32_t len;
  SpecializeMemory(T ptr_, uint32_t len_) : ptr(ptr_), len(len_) {}
  SpecializeMemory(const SpecializeMemory& other) = default;
};

namespace impl {
template <typename Ret, typename... Args>
using FuncPtr = Ret (*)(Args...);

template <typename T>
struct StoreArg;

template <>
struct StoreArg<uint32_t> {
  bool operator()(ArgWriter& args, uint32_t value) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_i32;
    arg.u.raw = 0;
    arg.u.i32 = value;
    return args.write(arg);
  }
};
template <>
struct StoreArg<bool> {
  bool operator()(ArgWriter& args, bool value) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_i32;
    arg.u.raw = 0;
    arg.u.i32 = value ? 1 : 0;
    return args.write(arg);
  }
};
template <>
struct StoreArg<uint64_t> {
  bool operator()(ArgWriter& args, uint64_t value) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_i64;
    arg.u.raw = 0;
    arg.u.i64 = value;
    return args.write(arg);
  }
};
template <>
struct StoreArg<float> {
  bool operator()(ArgWriter& args, float value) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_f32;
    arg.u.raw = 0;
    arg.u.f32 = value;
    return args.write(arg);
  }
};
template <>
struct StoreArg<double> {
  bool operator()(ArgWriter& args, double value) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_f64;
    arg.u.raw = 0;
    arg.u.f64 = value;
    return args.write(arg);
  }
};
template <typename T>
struct StoreArg<T*> {
  bool operator()(ArgWriter& args, T* value) {
    static_assert(sizeof(T*) == 4, "Only 32-bit Wasm supported");
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_i32;
    arg.u.raw = 0;
    arg.u.i32 = reinterpret_cast<uint32_t>(value);
    return args.write(arg);
  }
};
template <typename T>
struct StoreArg<T&> {
  bool operator()(ArgWriter& args, T& value) {
    return StoreArg<T*>(args, &value);
  }
};
template <typename T>
struct StoreArg<const T*> {
  bool operator()(ArgWriter& args, const T* value) {
    static_assert(sizeof(const T*) == 4, "Only 32-bit Wasm supported");
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_i32;
    arg.u.raw = 0;
    arg.u.i32 = reinterpret_cast<uint32_t>(value);
    return args.write(arg);
  }
};

template <typename... Args>
struct StoreArgs {};

template <>
struct StoreArgs<> {
  bool operator()(ArgWriter& args) { return true; }
};

template <typename T, typename... Rest>
struct StoreArgs<Specialize<T>, Rest...> {
  bool operator()(ArgWriter& args, Specialize<T> arg0, Rest... rest) {
    if (!StoreArg<T>()(args, arg0.value)) {
      return false;
    }
    return StoreArgs<Rest...>()(args, rest...);
  }
};

template <typename T, typename... Rest>
struct StoreArgs<SpecializeMemory<T>, Rest...> {
  bool operator()(ArgWriter& args, SpecializeMemory<T> arg0, Rest... rest) {
    weval_req_arg_t arg;
    arg.specialize = 1;
    arg.ty = weval_req_arg_buffer;
    arg.u.raw = 0;
    arg.u.buffer.len = arg0.len;
    arg.u.buffer.padded_len = (arg0.len + 7) & ~7;  // Align to 8-byte boundary.
    if (!args.write(arg)) {
      return false;
    }
    const uint8_t* src = reinterpret_cast<const uint8_t*>(arg0.ptr);
    uint8_t* dst = args.alloc(arg.u.buffer.padded_len);
    if (!dst) {
      return false;
    }
    memcpy(dst, src, arg0.len);
    if (arg.u.buffer.padded_len > arg.u.buffer.len) {
      // Ensure deterministic (zeroed) padding bytes.
      memset(dst + arg.u.buffer.len, 0,
             arg.u.buffer.padded_len - arg.u.buffer.len);
    }
    return StoreArgs<Rest...>()(args, rest...);
  }
};

template <typename T, typename... Rest>
struct StoreArgs<RuntimeArg<T>, Rest...> {
  bool operator()(ArgWriter& args, RuntimeArg<T> arg0, Rest... rest) {
    weval_req_arg_t arg;
    arg.specialize = 0;
    arg.ty = weval_req_arg_none;
    arg.u.raw = 0;
    if (!args.write(arg)) {
      return false;
    }
    return StoreArgs<Rest...>()(args, rest...);
  }
};

}  // namespace impl

template <typename Ret, typename... Args, typename... WrappedArgs>
weval_req_t* weval(impl::FuncPtr<Ret, Args...>* dest,
                   impl::FuncPtr<Ret, Args...> generic, uint32_t func_id,
                   WrappedArgs... args) {
  weval_req_t* req = (weval_req_t*)malloc(sizeof(weval_req_t));
  if (!req) {
    return nullptr;
  }
  ArgWriter writer;
  if (!impl::StoreArgs<WrappedArgs...>()(writer, args...)) {
    return nullptr;
  }

  req->func_id = func_id;
  req->func = (weval_func_t)generic;
  req->arglen = writer.len;
  req->argbuf = writer.take();
  req->specialized = (weval_func_t*)dest;

  weval_request(req);

  return req;
}

inline void free(weval_req_t* req) { weval_free(req); }

}  // namespace weval

#endif  // __cplusplus
