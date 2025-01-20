## Lynn_tcp

`Lynn_tcp` is a TCP service based on [Tokio](https://github.com/tokio-rs/tokio), and this project library is designed for easy use in different projects based on lynn_tcp. It may not be suitable for your own project.Anyway, you can use it for free, provided that you have a clear understanding of some of the customized attributes inside.

`Lynn_tcp`是一个基于[Tokio](https://github.com/tokio-rs/tokio)的tcp服务，这个项目库是为了在基于`Lynn_tcp`的不同项目中易于使用而设计的。它可能不适合你自己的项目。不管怎样，你可以免费使用它，前提是您要清楚地了解其中的一些定制化属性(如一些最大链接数等)。

![](https://camo.githubusercontent.com/6581c31c16c1b13ddc2efb92e2ad69a93ddc4a92fd871ff15d401c4c6c9155a4/68747470733a2f2f696d672e736869656c64732e696f2f62616467652f6c6963656e73652d4d49542d626c75652e737667)

### Simple Use

#### Dependencies

Make sure you activated the features which you need of the lynn_tcp on Cargo.toml:

**full features**

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main" }
```

**server feature**

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main", features = "server" }
```

**client feature**

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main", features = "client" }
```

#### Server

```rust
use lynn_tcp::server::{HandlerResult, InputBufVO, LynnServerConfigBuilder, LynnServer};

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

#### Server with config

```rust
use lynn_tcp::server::{HandlerResult, InputBufVO, LynnServerConfigBuilder, LynnServer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_server_ipv4("0.0.0.0:9177")
            .with_server_max_connections(Some(&200))
            .with_server_max_threadpool_size(&10)
      			// ...more
            .build(),
    )
    .await
    .add_router(1, my_service)
    .start()
    .await;
    Ok(())
}

pub fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
    println!("service read from :{}", input_buf_vo.get_input_addr());
    HandlerResult::new_without_send()
}
```

### Features

- `server`:A customized TCP service

- `client`:A customized TCP client

### License

This project is licensed under the MIT license.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Tokio by you, shall be licensed as MIT, without any additional terms or conditions.
