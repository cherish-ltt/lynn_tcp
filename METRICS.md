# Lynn TCP 监控指南

本指南介绍如何使用 Prometheus + Grafana 监控 Lynn TCP 框架。

## 快速开始

### 1. 启用 Metrics 功能

在 `Cargo.toml` 中启用 `metrics` feature：

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["server", "metrics"] }
```

### 2. 运行示例

```bash
cargo run --example metrics_example --features metrics
```

示例会在 `http://localhost:9091/metrics` 暴露 Prometheus 指标。

### 3. 配置 Prometheus

1. 下载 Prometheus：
```bash
wget https://github.com/prometheus/prometheus/releases/download/v2.45.0/prometheus-2.45.0.linux-amd64.tar.gz
tar xvf prometheus-2.45.0.linux-amd64.tar.gz
cd prometheus-2.45.0.linux-amd64
```

2. 使用项目提供的配置文件：
```bash
cp /path/to/lynn_tcp/docs/monitoring/prometheus/prometheus.yml .
./prometheus --config.file=prometheus.yml
```

3. 访问 Prometheus UI：http://localhost:9090

### 4. 配置 Grafana

1. 下载并启动 Grafana：
```bash
# macOS
brew install grafana
brew services start grafana

# Linux
sudo systemctl start grafana-server
```

2. 访问 Grafana：http://localhost:3000
   - 默认用户名/密码：admin/admin

3. 添加 Prometheus 数据源：
   - Configuration → Data Sources → Add data source
   - 选择 "Prometheus"
   - URL: `http://localhost:9090`
   - 点击 "Save & Test"

4. 导入 Dashboard：
   - Dashboards → Import
   - Upload `docs/monitoring/grafana/dashboard.json` 或粘贴内容
   - 选择 Prometheus 数据源
   - 点击 "Import"

## 可用指标

### 连接指标

| 指标 | 类型 | 描述 |
|------|------|------|
| `lynn_connections_total` | Counter | 总连接数 |
| `lynn_connections_active` | Gauge | 当前活跃连接数 |
| `lynn_connections_closed_total` | Counter | 总关闭连接数 |
| `lynn_connections_failed_total` | Counter | 失败连接数 |

### 消息指标

| 指标 | 类型 | 描述 |
|------|------|------|
| `lynn_messages_received_total` | Counter | 接收消息总数 |
| `lynn_messages_sent_total` | Counter | 发送消息总数 |
| `lynn_messages_invalid_total` | Counter | 无效消息数 |
| `lynn_messages_dropped_total` | Counter | 丢弃消息数 |
| `lynn_message_size_bytes` | Histogram | 消息大小分布 |
| `lynn_message_processing_duration_seconds` | Histogram | 消息处理时间 |

### 网络指标

| 指标 | 类型 | 描述 |
|------|------|------|
| `lynn_network_bytes_received_total` | Counter | 接收字节数 |
| `lynn_network_bytes_sent_total` | Counter | 发送字节数 |
| `lynn_network_tcp_retransmits_total` | Counter | TCP 重传次数 |
| `lynn_network_tcp_errors_total` | Counter | TCP 错误数 |

### 系统指标

| 指标 | 类型 | 描述 |
|------|------|------|
| `lynn_system_memory_used_bytes` | Gauge | 内存使用量 |
| `lynn_system_active_threads` | Gauge | 活跃线程数 |
| `lynn_system_queue_size` | Gauge | 任务队列长度 |

### 错误指标

| 指标 | 类型 | 标签 | 描述 |
|------|------|------|------|
| `lynn_errors_total` | Counter | error_type | 按类型统计的错误数 |
| `lynn_rate_limit_rejected_total` | Counter | 速率限制拒绝次数 |
| `lynn_validation_errors_total` | Counter | 验证错误次数 |

## 自动采集边界与迁移

- 仅采集服务端；客户端、指标 HTTP 监听器和业务自定义指标不混入内置计数。同进程多个服务端累计到进程级全局指标。
- `connections.total/active` 在 TCP accept 或通过 KCP 首个会话握手包校验后立即增加，包含等待准入、握手和分派的连接；配额所有权释放时 `active` 恰好减一、`closed_total` 恰好加一。准入拒绝、握手失败/超时及初始化阶段取消计入 `failed_total`；已建立会话的后续 I/O 错误不计为初始化失败。
- `messages.received_total` 在完整有界帧解析成功时增加，包含心跳、未知路由/构造类型；`size_bytes` 记录完整帧大小（含 10 字节头及 2 字节尾）。错误帧计入 `invalid_total`，不计入接收帧数；未完成帧 EOF 不算已解析错误帧。
- `messages.sent_total` 仅在每个目标响应完整写入并刷新后增加，不代表对端确认。广播按实际目标分别计数。
- `messages.dropped_total` 记录可观测到的处理许可拒绝、未知路由/构造类型、目标缺失、入队失败或响应写入失败。它不是所有丢弃帧的穷尽计数：owner 强制取消时尚在队列中的响应不逐项排空统计。
- `processing_duration_seconds` 记录 handler 执行时间，不含等待 worker、响应入队与网络发送；panic 或取消也通过计时器释放记录截至终止时的耗时。
- `network.bytes_*` 在成功的传输读写返回时累计真实字节数，包含帧头尾、错误输入和部分写入；Pending、EOF、失败及取消不会把请求写入长度误计为成功长度。TLS 计量的是解密后/加密前框架字节；KCP 计量适配字节流读写（发送计数可能先于 ACK，消息 sent 仍等待 flush）。不包含握手、KCP 会话封装/ACK/重传、TLS 记录开销或 IP/TCP/UDP 头。
- `tcp_retransmits_total`、`tcp_errors_total`、系统内存/线程/队列指标未自动采集，需调用方通过独立采集器赋值；不能用默认零值判断 OS 或进程健康。本文中的额外错误/限流指标名称是自定义采集约定，框架不会自动注册它们。
- exporter 首次调用即初始化内置指标；未产生流量时正常导出零值。

### 从手工统计迁移

删除旧 handler/示例中对上述自动指标的 `inc`、`inc_by`、`observe`，否则会双计数；公共 `METRICS` API 保持可用，但不再用于重复记录框架拥有的事件。当前不提供关闭自动采集的手工模式开关；业务指标应使用独立命名（例如 `myapp_orders_total`），由自己的 Prometheus 注册表/默认注册表管理。

`cargo run --example metrics_example` 提供真实 echo 与 exporter，不伪造连接、内存或线程数据。只启用 `metrics` 而不启用 `server` 时，仅提供注册和导出 API，不运行业务传输采集。

## PromQL 查询示例

### 查询活跃连接数

```
lynn_connections_active
```

### 查询每秒消息速率

```
rate(lynn_messages_received_total[1m])
```

### 查询 P95 消息处理延迟

```
histogram_quantile(0.95, rate(lynn_message_processing_duration_seconds_bucket[5m]))
```

### 查询错误率

```
sum(rate(lynn_errors_total[5m])) / sum(rate(lynn_messages_received_total[5m]))
```

### 查询网络吞吐量

```
sum(rate(lynn_network_bytes_received_total[1m])) + sum(rate(lynn_network_bytes_sent_total[1m]))
```

## 告警规则示例

创建 `alerts/lynn_tcp.yml`：

```yaml
groups:
  - name: lynn_tcp_alerts
    interval: 30s
    rules:
      # 高错误率告警
      - alert: HighErrorRate
        expr: |
          sum(rate(lynn_errors_total[5m])) /
          sum(rate(lynn_messages_received_total[5m])) > 0.05
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High error rate detected"
          description: "Error rate is {{ $value | humanizePercentage }}"

      # 连接数告警
      - alert: TooManyConnections
        expr: lynn_connections_active > 1000
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Too many active connections"
          description: "{{ $value }} active connections"

      # 高延迟告警
      - alert: HighLatency
        expr: |
          histogram_quantile(0.95,
            rate(lynn_message_processing_duration_seconds_bucket[5m])) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High message processing latency"
          description: "P95 latency is {{ $value }}s"

      # 内存使用告警
      - alert: HighMemoryUsage
        expr: lynn_system_memory_used_bytes > 1073741824  # 1GB
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "High memory usage"
          description: "Memory usage is {{ $value | humanize }}"
```

## Docker Compose 部署

创建 `docker-compose.yml`：

```yaml
version: '3.8'

services:
  lynn_tcp:
    build: .
    ports:
      - "9177:9177"  # TCP server
      - "9091:9091"  # Metrics
    environment:
      - RUST_LOG=info
    restart: unless-stopped

  prometheus:
    image: prom/prometheus:latest
    ports:
      - "9090:9090"
    volumes:
      - ./docs/monitoring/prometheus/prometheus.yml:/etc/prometheus/prometheus.yml
      - prometheus_data:/prometheus
    command:
      - '--config.file=/etc/prometheus/prometheus.yml'
      - '--storage.tsdb.path=/prometheus'
    restart: unless-stopped

  grafana:
    image: grafana/grafana:latest
    ports:
      - "3000:3000"
    volumes:
      - grafana_data:/var/lib/grafana
      - ./docs/monitoring/grafana/provisioning:/etc/grafana/provisioning
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=admin
    restart: unless-stopped

volumes:
  prometheus_data:
  grafana_data:
```

启动：

```bash
docker-compose up -d
```

## 性能影响

Metrics 功能的性能影响：

- **内存开销**：约 1-2 MB（指标存储）
- **CPU 开销**：<1%（指标更新）
- **网络开销**：每次 scrape 约 10-50 KB（取决于指标数量）

## 最佳实践

1. **合理的 scrape 间隔**：生产环境建议 10-30 秒
2. **使用 Histogram buckets**：根据实际数据分布调整
3. **添加标签**：为指标添加有意义的标签（如 server_id, region）
4. **定期审查指标**：移除不需要的指标
5. **配置告警**：设置合理的告警阈值
6. **保留数据**：根据需求调整 Prometheus 数据保留时间

## 故障排查

### Metrics 端点无法访问

1. 检查端口是否被占用：
```bash
lsof -i :9091
```

2. 检查防火墙设置

3. 查看应用日志是否启动 metrics server

### Prometheus 无法抓取指标

1. 检查 Prometheus 配置中的 target 地址

2. 在 Prometheus UI 中检查 target 状态：
   - http://localhost:9090/targets

3. 测试手动访问：
```bash
curl http://localhost:9091/metrics
```

### Grafana 无数据显示

1. 检查数据源连接状态

2. 确认时间范围设置

3. 手动运行 PromQL 查询测试

## 更多资源

- [Prometheus 文档](https://prometheus.io/docs/)
- [Grafana 文档](https://grafana.com/docs/)
- [PromQL 速查表](https://promlabs.com/promql-cheat-sheet/)
- [Grafana Dashboards](https://grafana.com/grafana/dashboards/)
