use std::ffi::c_void;
use std::sync::mpsc;
use std::alloc::alloc;
use std::alloc::dealloc;
use std::alloc::Layout;

use libffi::low;
use libffi::middle;

pub type TscStatus = u8;
pub const TSC_OK: TscStatus = 0;
pub const TSC_RECV_ERROR: TscStatus = 1;

#[derive(Debug)]
pub struct ThreadSafeCallback<'a> {
    pub closure: Option<middle::Closure<'a>>,
    pub args: Vec<Type>,
    pub ret: Type,
    pub sender: mpsc::Sender<ThreadSafeCallbackContext>,
    pub receiver: mpsc::Receiver<ThreadSafeCallbackContext>,
}

#[derive(Debug)]
pub struct ThreadSafeCallbackContext {
    pub args: Vec<*const c_void>,
    pub sender: mpsc::Sender<(*mut u8, *const c_void)>,
}

pub type Type = u8;

#[no_mangle]
pub unsafe extern "C" fn tsc_create(
    argv: usize,
    argc: *const Type,
    ret: Type,
    out: *mut *mut ThreadSafeCallback<'static>
) {
    let args = std::slice::from_raw_parts(argc, argv);
    let (sender, receiver) = mpsc::channel();
    let tsc_ptr = alloc(Layout::new::<ThreadSafeCallback<'static>>()) as *mut ThreadSafeCallback<'static>;
    let mut tsc = ThreadSafeCallback {
        closure: None,
        args: args.to_vec(),
        ret,
        sender,
        receiver,
    };
    let closure = middle::Closure::new(
        middle::Cif::new(
            args.iter().map(|ty| {
                match *ty {
                    0 => middle::Type::pointer(),
                    _ => panic!("unsupported argument type {}", ty),
                }
            }),
            match ret {
                0 => middle::Type::pointer(),
                1 => middle::Type::void(),
                _ => panic!("unsupported return type {}", ret),
            },
        ),
        tsc_callback,
        std::mem::transmute(tsc_ptr),
    );
    tsc.closure = Some(closure);
    tsc_ptr.write(tsc);
    *out = tsc_ptr;
}

#[no_mangle]
pub unsafe extern "C" fn tsc_next(
    tsc: *const ThreadSafeCallback<'static>,
    out: *mut *const ThreadSafeCallbackContext
) -> TscStatus {
    let tsc = &*tsc;
    let context = tsc.receiver.recv();
    match context {
        Ok(ctx) => {
            let ctx_ptr = alloc(Layout::new::<ThreadSafeCallbackContext>()) as *mut ThreadSafeCallbackContext;
            ctx_ptr.write(ctx);
            *out = ctx_ptr;
            TSC_OK
        },
        Err(_) => {
            TSC_RECV_ERROR
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn tsc_ptr(
    tsc: *const ThreadSafeCallback<'static>,
    out: *mut *const c_void
) {
    let tsc = &*tsc;
    *out = std::mem::transmute(*(tsc.closure.as_ref().unwrap().code_ptr()));
}

#[no_mangle]
pub unsafe extern "C" fn tsc_delete(tsc: *mut ThreadSafeCallback<'static>) {
    dealloc(tsc as *mut u8, Layout::new::<ThreadSafeCallback<'static>>());
}

#[no_mangle]
pub unsafe extern "C" fn tsc_ctx_args(
    ctx: *const ThreadSafeCallbackContext,
    buf: *mut *const c_void,
) {
    let ctx = &*ctx;
    let buf = std::slice::from_raw_parts_mut(buf, ctx.args.len());
    for (i, arg) in buf.iter_mut().enumerate() {
        *arg = ctx.args[i];
    }
}

#[no_mangle]
pub unsafe extern "C" fn tsc_ctx_return(
    ctx: *mut ThreadSafeCallbackContext,
    value: *const c_void,
) {
    let ptr = ctx as *mut _;
    let ctx = &*ctx;
    ctx.sender.send((ptr, value)).unwrap();
}

#[no_mangle]
pub unsafe extern "C" fn tsc_ctx_delete(ctx: *mut ThreadSafeCallbackContext) {
    dealloc(ctx as *mut u8, Layout::new::<ThreadSafeCallbackContext>());
}

unsafe extern "C" fn tsc_callback(
    _cif: &low::ffi_cif,
    result: &mut *const c_void,
    args: *const *const c_void,
    tsc: &ThreadSafeCallback,
) {
    let args = std::slice::from_raw_parts(args, tsc.args.len());
    let (sender, receiver) = mpsc::channel();
    let context = ThreadSafeCallbackContext {
        args: args.iter().enumerate().map(|(i, arg)| {
            let ty = tsc.args[i];
            match ty {
                0 => *(*arg as *const *const c_void),
                _ => panic!("unsupported type {}", ty),
            }
        }).collect(),
        sender,
    };
    tsc.sender.send(context).unwrap();
    // 1 is void so we'll not wait for return value in that case.
    if tsc.ret != 1 {
        let (ptr, ret) = receiver.recv().unwrap();
        dealloc(ptr, Layout::new::<ThreadSafeCallbackContext>());
        *result = ret;
    }
}
