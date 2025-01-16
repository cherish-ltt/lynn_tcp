## Lynn_tcp

`Lynn_tcp` is a TCP service based on `Tokio`, and this project library is designed for easy use in different projects based on lynn_tcp. It may not be suitable for your own project.Anyway, you can use it for free, provided that you have a clear understanding of some of the customized attributes inside.

`Lynn_tcp`是一个基于`Tokio`的tcp服务，这个项目库是为了在基于`Lynn_tcp`的不同项目中易于使用而设计的。它可能不适合你自己的项目。不管怎样，您就可以免费使用它，前提是您要清楚地了解其中的一些定制化属性(如一些最大链接数等)。

### Simple Use|如何使用

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main" }
```

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = LynnServer::new().await.add_router(1, my_service).start().await;
    Ok(())
}

pub fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
    println!("service read from :{}", input_buf_vo.get_input_addr());
    HandlerResult::new_without_send()
}
```

