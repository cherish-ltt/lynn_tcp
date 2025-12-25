# Lynn TCP 监控指南

本指南介绍如何使用 Prometheus + Grafana 监控 Lynn TCP 框架。

## 快速开始

### 1. 启用 Metrics 功能

在 `Cargo.toml` 中启用 `metrics` feature：

```toml
[dependencies]
lynn_tcp = { version = "1.3", features = ["server", "metrics"] }
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
cp /path/to/lynn_tcp/prometheus/prometheus.yml .
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
   - Upload `grafana/dashboard.json` 或粘贴内容
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

## 在代码中使用 Metrics

### 基础使用

```rust
use lynn_tcp::lynn_metrics::*;

// 启动 metrics server
let config = MetricsServerConfig {
    bind_addr: "0.0.0.0:9091".to_string(),
    enabled: true,
};
let _handle = spawn_metrics_server(config);

// 记录指标
METRICS.connections.total.inc();
METRICS.connections.active.inc();
METRICS.messages.received_total.inc();
```

### 在 Handler 中使用

```rust
pub async fn my_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    // 记录消息接收
    METRICS.messages.received_total.inc();

    // 记录消息大小
    if let Some(data) = input_buf_vo.get_all_bytes().len() {
        METRICS.messages.size_bytes.observe(data as f64);
    }

    // 使用 Timer 记录处理时间
    METRICS.messages.processing_duration_seconds.observe_duration(|| {
        // 处理消息...
        HandlerResult::new_without_send()
    })
}
```

### 错误跟踪

```rust
match parse_message(data) {
    Ok(msg) => {
        METRICS.messages.received_total.inc();
        // ...
    }
    Err(e) => {
        METRICS.errors.total
            .with_label_values(&["parse_error"])
            .inc();
        METRICS.messages.invalid_total.inc();
    }
}
```

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
      - ./prometheus/prometheus.yml:/etc/prometheus/prometheus.yml
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
      - ./grafana/provisioning:/etc/grafana/provisioning
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
