# 横向支撑 · Terminal text
> Pure terminal-control escaping shared by independent CLI products.

## 地位
OS-adjacent output primitive; standard library only, no Agent/product/credentials policy or terminal actions.
## 逻辑
Escape C0/C1 controls (including ESC, CR, DEL/CSI/OSC) into visible Unicode escapes while preserving ordinary UTF-8, LF and TAB. CLI consumers apply it only at display/write boundaries, never to model history, executed arguments or original artifacts.
## 约束
Stateless text transformation, no environment/TTY/clipboard access. Invalid UTF-8 follows Go rune replacement; sources are expected to deliver validated UTF-8 text chunks. This is not arbitrary-secret detection. Consumers own byte/output limits, cancellation, real execution and write failures.
## 业务域清单
| 文件 | 职责 |
|---|---|
| text.go / text_test.go | Shared control escaping, Unicode/newline/tab preservation and immutable-input regressions |
