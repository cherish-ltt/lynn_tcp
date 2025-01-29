use std::{future::Future, pin::Pin};

use crate::{
    handler::{HandlerContext, SystemParam, SystemParamFunction, SystemParamState},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};

macro_rules! impl_system_param_function {
    ($($ty:ident),*) => {
        impl<T,Fut,$($ty,)*> SystemParamFunction<($($ty,)*)> for T
        where
            T: Fn($($ty,)*) -> Fut  +  Send+Sync+'static,
            Fut: Future<Output = HandlerResult> + Send+'static,
            $( $ty: SystemParam + Send, )*
        {
            // type Fut=Fut;
            fn run(&self,($($ty,)*):($($ty,)*)) -> Pin<Box<dyn Future<Output = HandlerResult>+Send+'static>> {
                Box::pin((self)($($ty,)*))
            }
        }
    };
}

#[rustfmt::skip]
macro_rules! impl_tuple {
    ($name:ident) => {
        $name!();
        $name!(T1);
        $name!(T1, T2);
        $name!(T1, T2, T3);
        $name!(T1, T2, T3, T4);
        $name!(T1, T2, T3, T4, T5);
        $name!(T1, T2, T3, T4, T5, T6);
        $name!(T1, T2, T3, T4, T5, T6, T7);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14);
        $name!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15);
    };
}

macro_rules! impl_system_param {
    ($($param: ident),*) => {
        impl<$($param: SystemParam,)*> SystemParam
            for ($($param,)*)
        {
            type State = ($($param::State,)*);
        }
    };
}

macro_rules! impl_system_param_state {
    ($($param: ident),*) => {
        impl<$($param),*> SystemParamState for ($($param,)*)
        where
            $($param: SystemParamState,)*
        {
            type Item = ($($param::Item,)*);

            fn init() -> Self{
                ($($param::init(),)*)
            }

            fn get_param(
                state: &Self,
                context: &HandlerContext
            ) -> Self::Item{
                let ($($param,)*) = state;
                ($($param::get_param($param,context),)*)
            }
        }
    };
}

impl_tuple!(impl_system_param_function);
impl_tuple!(impl_system_param);
impl_tuple!(impl_system_param_state);
