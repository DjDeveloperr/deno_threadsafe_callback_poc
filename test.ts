const tsc = Deno.dlopen(
  "./target/debug/deno_threadsafe_cb.dll",
  {
    tsc_create: {
      parameters: ["usize", "pointer", "u8", "pointer"],
      result: "void",
    },

    tsc_next: {
      nonblocking: true,
      parameters: ["pointer", "pointer"],
      result: "u8",
    },

    tsc_ptr: {
      parameters: ["pointer", "pointer"],
      result: "void",
    },

    tsc_delete: {
      parameters: ["pointer"],
      result: "void",
    },

    tsc_ctx_args: {
      parameters: ["pointer", "pointer"],
      result: "void",
    },

    tsc_ctx_return: {
      parameters: ["pointer", "pointer"],
      result: "void",
    },

    tsc_ctx_delete: {
      parameters: ["pointer"],
      result: "void",
    },
  } as const,
).symbols;

export class ThreadSafeCallback<Fn extends Deno.ForeignFunction> {
  handle: Deno.UnsafePointer;
  pointer: Deno.UnsafePointer;

  constructor(public def: Fn) {
    const out = new BigUint64Array(1);
    tsc.tsc_create(
      def.parameters.length,
      new Uint8Array(def.parameters.map((e) => {
        if (e === "pointer") {
          return 0;
        } else {
          throw new Error("Unsupported type");
        }
      })),
      def.result === "void" ? 1 : def.result === "pointer" ? 0 : 2,
      out,
    );
    this.handle = new Deno.UnsafePointer(out[0]);
    const outPointer = new BigUint64Array(1);
    tsc.tsc_ptr(this.handle, outPointer);
    this.pointer = new Deno.UnsafePointer(outPointer[0]);
  }

  async next() {
    const out = new BigUint64Array(1);
    const res = await tsc.tsc_next(this.handle, out);
    if (res !== 0) {
      return;
    } else {
      const ptr = new Deno.UnsafePointer(out[0]);
      return new ThreadSafeCallbackContext(ptr, this.def.parameters.length);
    }
  }

  delete() {
    tsc.tsc_delete(this.handle);
  }

  async *[Symbol.asyncIterator]() {
    while (true) {
      const ctx = await this.next();
      if (ctx) {
        yield ctx;
      } else {
        break;
      }
    }
  }
}

export class ThreadSafeCallbackContext {
  handle: Deno.UnsafePointer;
  readonly arguments: any[];

  constructor(handle: Deno.UnsafePointer, argc: number) {
    this.handle = handle;
    this.arguments = new Array(argc).fill(undefined);
    const ptrs = new BigUint64Array(argc);
    tsc.tsc_ctx_args(this.handle, ptrs);
    for (let i = 0; i < argc; i++) {
      this.arguments[i] = new Deno.UnsafePointer(ptrs[i]);
    }
  }

  return(value: Deno.UnsafePointer) {
    tsc.tsc_ctx_return(this.handle, value);
  }

  delete() {
    tsc.tsc_ctx_delete(this.handle);
  }
}

// Callback with a return type, run on different thread
{
  const cb = new ThreadSafeCallback(
    {
      parameters: ["pointer", "pointer"],
      result: "pointer",
    } as const,
  );

  // We need to call it from other thread so that it
  // doesn't dead lock.
  const fnptr = new Deno.UnsafeFnPointer<
    typeof cb.def & { nonblocking: true }
  >(
    cb.pointer,
    Object.assign(
      {
        nonblocking: true,
      } as const,
      cb.def,
    ),
  );

  cb.next().then((ctx) => {
    if (!ctx) return;
    console.log("args", ctx.arguments);
    ctx.return(new Deno.UnsafePointer(123n));
  });

  const ptr = await fnptr.call(
    new Deno.UnsafePointer(6n),
    new Deno.UnsafePointer(9n),
  );
  console.log("return", ptr);
}

// Callback without a return type, run on same thread
{
  const cb = new ThreadSafeCallback(
    {
      parameters: ["pointer", "pointer"],
      result: "void",
    } as const,
  );

  // We need to call it from other thread so that it
  // doesn't dead lock.
  const fnptr = new Deno.UnsafeFnPointer(cb.pointer, cb.def);

  cb.next().then((ctx) => {
    if (!ctx) return;
    console.log("args", ctx.arguments);
  });

  fnptr.call(
    new Deno.UnsafePointer(1n),
    new Deno.UnsafePointer(2n),
  );
}
