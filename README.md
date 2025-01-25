## Lynn_tcp

[![Crates.io](https://img.shields.io/crates/v/lynn_tcp)](https://crates.io/crates/lynn_tcp)  [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cherish-ltt/lynn_tcp/blob/main/LICENSE)

English|[简体中文](https://github.com/cherish-ltt/lynn_tcp/blob/main/README_ZH.md)

`Lynn_tcp` is a lightweight TCP server framework

------

### Keywords

- **Lightweight**: concise code that is easier to learn and use

- **Concurrent and Performance**: Based on Tokio's excellent asynchronous performance, it is easy to achieve concurrent processing capabilities for multi-user links

- **Lower latency**: Design with read and write separation to achieve lower latency

- **Security**: Code written with strong typing and memory safety in Rust

  > **tips**: Lynn_tcp is mainly used for <u>message forwarding</u> and <u>long link TCP game servers</u>
  >
  > Quickly develop suitable business scenarios based on frameworks
  >
  > Different message parsing, communication data encryption, and other operations can be achieved through custom settings

### Simple Use

#### Dependencies

Make sure you activated the features which you need of the lynn_tcp on Cargo.toml:

**full features**

Use `cargo add lynn_tcp` or:

```rust
[dependencies]
lynn_tcp = "1.0.1"
```

**server feature**

```rust
[dependencies]
lynn_tcp = { version = "1.0.1" , features = "server" }
```

**client feature**

```rust
[dependencies]
lynn_tcp = { version = "1.0.1" , features = "client" }
```

#### Server

```rust
use lynn_tcp::{
    async_func_wrapper,
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::*,
};
use std::pin::Pin;
use std::future::Future;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = LynnServer::new().await.add_router(1, async_func_wrapper!(my_service)).start().await;
    Ok(())
}

pub async fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
    println!("service read from :{}", input_buf_vo.get_input_addr());
    HandlerResult::new_without_send()
}
```

#### Server with config

```rust
use lynn_tcp::{
    async_func_wrapper,
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::*,
};
use std::pin::Pin;
use std::future::Future;

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
    .add_router(1, async_func_wrapper!(my_service))
    .start()
    .await;
    Ok(())
}

pub async fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
    println!("service read from :{}", input_buf_vo.get_input_addr());
    HandlerResult::new_without_send()
}
```

#### Client

```rust
use lynn_tcp::{
    lynn_client::LynnClient,
    lynn_tcp_dependents::*,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = LynnClient::new_with_ipv4("127.0.0.1:9177")
            .await
            .start()
            .await;
    let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
    let input_buf_vo = client.get_receive_data().await.unwrap();
    Ok(())
}
```

### Features

- `server`: Provide customizable TCP services that can easily achieve multi-user long connections and concurrent processing capabilities, with services for different routes
- `client`: Provide custom TCP clients that can send messages to TCP servers and receive messages from servers, with different routing services

### Road maps

#### Basic functions

 ✅ Tcp server

 ✅ Tcp client

 ✅ Custom message parsing

 ✅ Automatically clean sockets

 ✅ Routing service for asynchronous tasks

> Note:
>
> All Basic functions support on v1.0.0 and above

#### Extended functions

- [ ] Scheduled tasks
- [ ] Middleware
- [ ] Global database handle
- [ ] Communication data encryption
- [ ] Disconnecting reconnection mechanism

> Note:
>
> If you haven't seen clear instructions, it's currently unavailable and still under development

### Flow chart

[FlowChart.png](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true)

![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true)

### Release note 

[version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/version.md)

### License

This project is licensed under the MIT license.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Lynn_tcp by you, shall be licensed as MIT, without any additional terms or conditions.
