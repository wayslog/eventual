//! 异步计算的基本组件
//!
//! 本模块包含一系列用于管理异步计算的工具
//! 这些工具大部分是基于`Future`和`Stream`这两个类型实现的。
//! 同时，这些类型的函数，也允许我们对计算进行合成。
//! （译者注：本文中计算原义为computation，才疏学浅，不知有什么更好的翻译，各位海涵。）
//!
//! ## Future
//!
//! `Future`是一个对计算结果的代理，这些计算可能是暂时未完成的。
//! 这些计算可能会被并行的跑在另一个线程里或者当计算完成时可以触发其异步回调函数。
//! 一种理解就是`Future`就是一个异步计算的结果，其值就是异步计算结果的值。
//! （译者注：作者是不是rust写多了，写文档的时候还带着unwrap……）
//!
//! For example:
//!
//! ```
//! use eventual::*;
//!
//! // 在一个线程里跑一个计算
//! let future1 = Future::spawn(|| {
//!     // 一个巨复杂无比的计算，当然，这里我们就小小的返回个数字
//!     42
//! });
//!
//! // 在另一个线程里跑计算
//! let future2 = Future::spawn(|| {
//!     // 另一个巨复杂无比的计算
//!     18
//! });
//! // 注意了，这里他们要合并了
//! let res = join((
//!         future1.map(|v| v * 2),
//!         future2.map(|v| v + 5)))
//!     .and_then(|(v1, v2)| Ok(v1 - v2))
//!     .await().unwrap();
//!
//! assert_eq!(61, res);
//!
//! ```
//!
//! ## Stream
//!
//! `Stream` 和`Future`差不多, 但是`Future`表达的是单独的一个计算的结果值，而`Stream`
//! 表达的是一个结果序列。
//!

#![crate_name = "eventual"]
#![deny(warnings)]

extern crate syncbox;
extern crate time;

#[macro_use]
extern crate log;

pub use self::future::{Future, Complete};
pub use self::join::{join, Join};
pub use self::receipt::Receipt;
pub use self::run::{background, defer};
pub use self::select::{select, Select};
pub use self::sequence::sequence;
pub use self::stream::{Stream, StreamIter, Sender, BusySender};
pub use self::timer::Timer;

use std::error::Error;
use std::fmt;

// ## TODO
//
// * Switch generics to where clauses
//   - rust-lang/rust#20300 (T::Foo resolution)
//
// * Allow Async::or & Async::or_else to change the error type
//
// * Improve performance / reduce allocations

mod core;
mod future;
mod join;
mod process;
mod receipt;
mod run;
mod select;
mod sequence;
mod stream;
mod timer;

/// A value representing an asynchronous computation
/// 每一个结果即代表一个异步计算
pub trait Async : Send + 'static + Sized {
    /// 译者读代码注：
    /// `Value`最后计算的结果类型
    type Value: Send + 'static;
    /// 译者读代码注：
    /// `Error`是这个计算过程中出错而抛出的自定义类型
    type Error: Send + 'static;
    /// 译者读代码注：
    /// `Cancel`顾名思义是一个用来取消计算结果的类型
    type Cancel: Cancel<Self>;

    /// 如果计算能成功则返回 true
    fn is_ready(&self) -> bool;

    /// 与`is_ready`对应，如果计算完成并且计算失败的时候则返回 true
    fn is_err(&self) -> bool;

    /// 如果底层有计算结果出现，无论成功或失败完成未完成，获得之
    fn poll(self) -> Result<AsyncResult<Self::Value, Self::Error>, Self>;

    /// 如果底层有计算结果出现，无论成功失败，获得之。如果计算还未完成，则会引发panic.
    fn expect(self) -> AsyncResult<Self::Value, Self::Error> {
        if let Ok(v) = self.poll() {
            return v;
        }

        panic!("the async value is not ready");
    }

    /// 如果计算结果能被消费了，则执行回调函数
    fn ready<F>(self, f: F) -> Self::Cancel where F: FnOnce(Self) + Send + 'static;

    /// 如果`AsyncResult`已经准备好了，调用回调函数来接收`AsyncResult`
    fn receive<F>(self, f: F)
            where F: FnOnce(AsyncResult<Self::Value, Self::Error>) + Send + 'static {
        self.ready(move |async| {
            match async.poll() {
                Ok(res) => f(res),
                Err(_) => panic!("ready callback invoked but is not actually ready"),
            }
        });
    }

    /// 阻塞当前线程直到`AsyncResult`被计算完成并且返回了结果
    fn await(self) -> AsyncResult<Self::Value, Self::Error> {
        use std::sync::mpsc::channel;

        let (tx, rx) = channel();

        self.receive(move |res| tx.send(res).ok().expect("receiver thread died"));
        rx.recv().ok().expect("async disappeared without a trace")
    }

    /// 触发计算，但是并不接收其计算结果
    fn fire(self) {
        self.receive(drop)
    }

    /*
     *
     * ===== Computation Builders =====
     *
     */

    /// 本method返回一个`Future`实例，结果依赖于原本的`Future`
    /// (译者注：可以认为就是调用者的self)的计算结果
    ///
    /// 如果原`Future`返回的结果是Err，则本method返回的`Future`的计算结果将是这个Error。
    ///
    /// 如果原`Future`返回的是Ok，那么由本method返回的`Future`的计算结果将是其原本的结果值
    ///
    /// 译者读代码注：
    ///
    /// 这里可以理解为指挥官-士兵的模型——
    /// 指挥官大脑里打了很多小算盘来算计是不是要排出士兵去攻击敌方阵地。
    ///
    /// 当指挥官觉得不行，这样做风险太大的时候，他抛出了一个错误(Err)，于是士兵们知道了指挥官不要他们出击，
    /// 但是士兵不会思考，他只会机械的将指挥官的错误告诉别人。
    ///
    /// 当指挥官觉得可行的时候，他会告诉士兵，你们可以去攻击了，由于士兵们都被提前设定好了攻击步骤，
    /// 于是士兵们得到命令就高高兴兴的出发了，战斗的结果的话，还是要看士兵的战斗力了哈。
    fn and<U: Async<Error=Self::Error>>(self, next: U) -> Future<U::Value, Self::Error> {
        self.and_then(move |_| next)
    }

    /// 本method返回一个`Future`实例，计算结果依赖于原本的`Future`
    ///
    /// 如果原`Future`返回的结果是Err，则本method返回的`Future`的计算结果将是这个Error。
    ///
    /// 如果原`Future`返回的是Ok，那么参数中的callback函数将会接收到这个值并且被执行，
    /// 并且这个callback函数将会返回一个异步结果(Async)
    /// 最后，本method返回的`Future`将会返回callback函数的计算结果。
    ///
    /// 译者读代码注：
    ///
    /// 好吧，我觉得还是有人不懂，那么在用一个模型来表达就是指挥官-队长-士兵的模型——
    ///
    /// 接着上一个模型来看，当指挥官的命令下达之后，队长将分析指挥官的命令，然后队长也经历了一个小波头脑风暴，
    ///
    /// 队长觉得可以，就上，最后的结果算我的。
    ///
    /// 队长觉得不可以，将在外军令有所不受，把指挥官的命令当擦屁股纸了。
    ///
    /// 士兵们表示我们是无辜的，上面说啥就是啥
    ///
    /// ```
    /// use eventual::*;
    ///
    /// let f = Future::of(1337);
    ///
    /// f.and_then(|v|
    ///     assert_eq!(v, 1337);
    ///     // 返回了一个正确的计算结果 Ok(1007)
    ///     Ok(1007)
    /// }).and_then(|v| {
    ///     // assert_eq!() 将返回一个unit_type,其值为()
    ///     // 至于why，看下面 `impl Async for ()`
    ///     assert_eq!(v, 1007)
    /// }).await();
    ///
    /// let e = Future::<(), &'static str>::error("failed");
    ///
    /// e.and_then(|v| {
    ///     panic!("unreachable");
    ///     Ok(())
    /// }).await();
    /// ```
    fn and_then<F, U: Async<Error=Self::Error>>(self, f: F) -> Future<U::Value, Self::Error>
            where F: FnOnce(Self::Value) -> U + Send + 'static,
                  U::Value: Send + 'static {
        let (complete, ret) = Future::pair();

        complete.receive(move |c| {
            if let Ok(complete) = c {
                self.receive(move |res| {
                    match res {
                        Ok(v) => {
                            f(v).receive(move |res| {
                                match res {
                                    Ok(u) => complete.complete(u),
                                    Err(AsyncError::Failed(e)) => complete.fail(e),
                                    _ => {}
                                }
                            });
                        }
                        Err(AsyncError::Failed(e)) => complete.fail(e),
                        _ => {}
                    }
                });
            }
        });

        ret
    }

    /// 本method返回一个`Future`实例，计算结果依赖于原本的`Future`
    /// 这个函数，简单来说就是`and`的反向逻辑，原函数给的Ok的结果，我接受并且返回，
    /// 原函数给的错误的结果，那么我就用我自己的`Future`去替代它。
    fn or<A>(self, alt: A) -> Future<Self::Value, A::Error>
            where A: Async<Value=Self::Value> {
        self.or_else(move |_| alt)
    }

    /// 本method返回一个`Future`实例，计算结果依赖于原本的`Future`
    /// 与`and_else`相反，这个函数在原`Future`返回一个Err的时候时候去调用callback函数，
    /// 处理并返回新结果。
    fn or_else<F, A>(self, f: F) -> Future<Self::Value, A::Error>
            where F: FnOnce(Self::Error) -> A + Send + 'static,
                  A: Async<Value=Self::Value> {

        let (complete, ret) = Future::pair();

        complete.receive(move |c| {
            if let Ok(complete) = c {
                self.receive(move |res| {
                    match res {
                        Ok(v) => complete.complete(v),
                        Err(AsyncError::Failed(e)) => {
                            f(e).receive(move |res| {
                                match res {
                                    Ok(v) => complete.complete(v),
                                    Err(AsyncError::Failed(e)) => complete.fail(e),
                                    _ => {}
                                }
                            });
                        }
                        Err(AsyncError::Aborted) => drop(complete),
                    }
                });
            }
        });

        ret
    }
}

pub trait Pair {
    type Tx;

    fn pair() -> (Self::Tx, Self);
}

pub trait Cancel<A: Send + 'static> : Send + 'static {
    fn cancel(self) -> Option<A>;
}

/*
 *
 * ===== Async implementations =====
 *
 */

impl<T: Send + 'static, E: Send + 'static> Async for Result<T, E> {
    type Value = T;
    type Error = E;
    type Cancel = Option<Result<T, E>>;

    fn is_ready(&self) -> bool {
        true
    }

    fn is_err(&self) -> bool {
        self.is_err()
    }

    fn poll(self) -> Result<AsyncResult<T, E>, Result<T, E>> {
        Ok(self.await())
    }

    fn ready<F: FnOnce(Result<T, E>) + Send + 'static>(self, f: F) -> Option<Result<T, E>> {
        f(self);
        None
    }

    fn await(self) -> AsyncResult<T, E> {
        self.map_err(|e| AsyncError::Failed(e))
    }
}

impl<A: Send + 'static> Cancel<A> for Option<A> {
    fn cancel(self) -> Option<A> {
        self
    }
}

/// Convenience implementation for (), to ease use of side-effecting functions returning unit
impl Async for () {
    type Value  = ();
    type Error  = ();
    type Cancel = Option<()>;

    fn is_ready(&self) -> bool {
        true
    }

    fn is_err(&self) -> bool {
        false
    }

    fn poll(self) -> Result<AsyncResult<(), ()>, ()> {
        Ok(Ok(self))
    }

    fn ready<F: FnOnce(()) + Send + 'static>(self, f: F) -> Option<()> {
        f(self);
        None
    }

    fn await(self) -> AsyncResult<(), ()> {
         Ok(self)
    }
}

/*
 *
 * ===== AsyncResult =====
 *
 */

pub type AsyncResult<T, E> = Result<T, AsyncError<E>>;

#[derive(Eq, PartialEq)]
pub enum AsyncError<E: Send + 'static> {
    Failed(E),
    Aborted,
}

impl<E: Send + 'static> AsyncError<E> {
    pub fn failed(err: E) -> AsyncError<E> {
        AsyncError::Failed(err)
    }

    pub fn aborted() -> AsyncError<E> {
        AsyncError::Aborted
    }

    pub fn is_aborted(&self) -> bool {
        match *self {
            AsyncError::Aborted => true,
            _ => false,
        }
    }

    pub fn is_failed(&self) -> bool {
        match *self {
            AsyncError::Failed(..) => true,
            _ => false,
        }
    }

    pub fn unwrap(self) -> E {
        match self {
            AsyncError::Failed(err) => err,
            AsyncError::Aborted => panic!("unwrapping a cancellation error"),
        }
    }

    pub fn take(self) -> Option<E> {
        match self {
            AsyncError::Failed(err) => Some(err),
            _ => None,
        }
    }
}

impl<E: Send + Error + 'static> Error for AsyncError<E> {
    fn description(&self) -> &str {
        match *self {
            AsyncError::Failed(ref e) => e.description(),
            AsyncError::Aborted => "aborted",
        }
    }

    fn cause(&self) -> Option<&Error> {
        match *self {
            AsyncError::Failed(ref e) => e.cause(),
            AsyncError::Aborted => None,
        }
    }
}

impl<E: Send + 'static + fmt::Debug> fmt::Debug for AsyncError<E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AsyncError::Failed(ref e) => write!(fmt, "AsyncError::Failed({:?})", e),
            AsyncError::Aborted => write!(fmt, "AsyncError::Aborted"),
        }
    }
}

impl<E: Send + 'static + fmt::Display> fmt::Display for AsyncError<E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AsyncError::Failed(ref e) => write!(fmt, "{}", e),
            AsyncError::Aborted => write!(fmt, "[aborted]"),
        }
    }
}

/*
 *
 * ===== BoxedReceive =====
 *
 */

// Needed to allow virtual dispatch to Receive
trait BoxedReceive<T> : Send + 'static {
    fn receive_boxed(self: Box<Self>, val: T);
}

impl<F: FnOnce(T) + Send + 'static, T> BoxedReceive<T> for F {
    fn receive_boxed(self: Box<F>, val: T) {
        (*self)(val)
    }
}
