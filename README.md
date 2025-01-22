## Lynn_tcp

![](https://camo.githubusercontent.com/6581c31c16c1b13ddc2efb92e2ad69a93ddc4a92fd871ff15d401c4c6c9155a4/68747470733a2f2f696d672e736869656c64732e696f2f62616467652f6c6963656e73652d4d49542d626c75652e737667)

`Lynn_tcp` is a lightweight TCP server framework

------

### Keywords

- **Lightweight**: concise code that is easier to learn and use

- **Concurrent and Performance**: Based on Tokio's excellent asynchronous performance, it is easy to achieve concurrent processing capabilities for multi-user links

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

- `server`: Provide customizable TCP services that can easily achieve multi-user long connections and concurrent processing capabilities, with services for different routes
- `client`: Provide custom TCP clients that can send messages to TCP servers and receive messages from servers, with different routing services

### Road maps

#### Basic functions

- [x] Tcp server

- [x] Tcp client

- [x] Custom message parsing

- [x] Automatically clean sockets

- [x] Routing service for synchronous tasks

> Note:
>
> All Basic functions support on v1.0.0 and above

#### Extended functions

- [ ] Scheduled tasks
- [ ] Middleware
- [ ] Global database handle
- [ ] Communication data encryption
- [ ] Routing service for asynchronous tasks
- [ ] Disconnecting reconnection mechanism

### Flow chart

![image](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png)

### Release note 

[version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/version.md)

### License

This project is licensed under the MIT license.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Lynn_tcp by you, shall be licensed as MIT, without any additional terms or conditions.
