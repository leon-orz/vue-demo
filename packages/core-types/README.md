# @toogl/core-types

前后端共享 TS 类型，**由 ts-rs 从 `crates/core/src/models` 自动生成，禁止手改**。

## 生成链路

```bash
cargo test -p toogl-core export_bindings   # 重新生成 bindings/
```

CI 会在 `cargo test` 后 `git diff --exit-code` 校验生成物与仓库一致。
