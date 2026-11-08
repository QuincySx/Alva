# 横向支撑 · System keychain
> 小密钥只进操作系统钥匙串，不落明文文件。

## 地位

底层 OS 原语，不认识 MCP、OAuth 或产品；调用方决定 service/account 命名与内容。

## 逻辑

darwin 用 purego 调 Security.framework 的 SecItem* 管理登录钥匙串里的通用密码项：Put 先 Update 后 Add，Delete 幂等，Exists 只匹配属性不取数据，减少未签名构建的系统确认。Memory 是进程内实现，供测试和明确不持久化的程序使用。

## 约束

单值上限 16 KiB，account/service 为可见 ASCII。非 darwin 平台 System 返回 ErrUnsupported，调用方不得回退到磁盘。真实钥匙串测试需显式 `ALVA_KEYCHAIN_TEST=1`。

## 业务域清单

| 名称 | 文件 | 职责 |
|---|---|---|
| Contract | `keychain.go` | Vault、System、Memory、错误与上限 |
| macOS | `keychain_darwin.go` | Security.framework 通用密码项 |
| Other | `keychain_other.go` | 明确不支持 |
| Regression | `keychain_test.go` | Memory 与（显式开启的）真实钥匙串往返、边界与缺失项 |
