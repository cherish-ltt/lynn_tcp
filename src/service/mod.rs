use crate::{dto_factory::input_dto::HandlerResult, vo_factory::input_vo::InputBufVO};

/// A trait for services that can process `InputBufVO` and return a `HandlerResult`.
///
/// This trait is used to define the behavior of services that can process `InputBufVO` and return a `HandlerResult`.
/// It requires that the implementing type be `Send` and `Sync`, which means it can be safely sent between threads and accessed concurrently.
pub(crate) trait IService: Send + Sync {
    /// Processes the given `InputBufVO` and returns a `HandlerResult`.
    ///
    /// This method takes a mutable reference to an `InputBufVO` and returns a `HandlerResult`.
    /// The implementing type should define the logic for processing the `InputBufVO` and returning the appropriate `HandlerResult`.
    fn service(&self, input_buf_vo: &mut InputBufVO) -> HandlerResult;
}

/// An implementation of the `IService` trait for any type that implements `Fn(&mut InputBufVO) -> HandlerResult` and is `Send` and `Sync`.
///
/// This implementation allows any type that implements `Fn(&mut InputBufVO) -> HandlerResult` and is `Send` and `Sync` to be used as an `IService`.
/// It simply calls the function with the given `InputBufVO` and returns the result.
impl<T> IService for T
where
    T: Fn(&mut InputBufVO) -> HandlerResult + Send + Sync + 'static + Sized,
{
    /// Processes the given `InputBufVO` by calling the function and returns the result.
    ///
    /// This method takes a mutable reference to an `InputBufVO` and calls the function with it.
    /// It then returns the `HandlerResult` returned by the function.
    fn service(&self, input_buf_vo: &mut InputBufVO) -> HandlerResult {
        (self)(input_buf_vo)
    }
}
