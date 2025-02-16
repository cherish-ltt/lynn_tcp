# Version Note

### v1.1.x - release

#### v1.1.3 - release

1.fix

- Heartbeat update mechanism:Under the previous heartbeat update mechanism, msg that did not match the tag would also be treated as the correct client. Now, only standard heartbeats are received to update the heartbeat, otherwise the client will be removed in the next heartbeat detection

#### v1.1.2 - release

1.fix

- Link management(DELAYED SEND)
- Log output adjustment

2.docs

- Supplement and modify doc

#### v1.1.1 - release

1.perf

- Overall performance optimization

2.fix

- Fix `LynnConfigBuilder` failed to export correctly

3.refactor

- Structural optimization and adjustment mainly focus on code readability and maintainability

4.docs

- Improve the crate documentation

5.redundancy

- Delete abandoned code

#### v1.1.0 - release

1.feat

- Support asynchronous function tasks with different parameter routing

### v1.0.x - release

#### v1.0.3 - release

1.fix

- verified sticky package bug

#### v1.0.2 - release

1.fix

- Several known bugs

#### v1.0.1 - release

1.docs

- Improve documentation

#### v1.0.0 - release

1.feat

- Tcp server

- Tcp client

- Custom message parsing

- Automatically clean sockets

- Routing service for synchronous tasks