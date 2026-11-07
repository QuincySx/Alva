# Native document text
> Bounded text-only PDF extraction through an explicitly owned OS worker.

## 地位
底层可选宿主原语；不依赖 Hub、SDK、数据库或外部账户。NewProcessor 冷构造，不启动进程或读取文件。

## 逻辑
调用方显式传入只读 regular file，最多 32 MiB。macOS 仅通过固定 `/usr/bin/osascript` JavaScriptObjC 程序调用系统 PDFKit；原件通过继承只读 fd 3 传入，无路径或用户源码插值，不发送 AppleEvents、不调用文档动作/脚本、不发现用户库。pageCount/pageAtIndex/string 按页提取实际文本，最多 512 页、2 MiB UTF-8、30 秒；扫描图像、加密锁定或没有可提取文字时真实失败，无 OCR/渲染/网络 fallback。

原生解析位于独立 owned process，两个全局子进程名额；使用实际 owned PID 的 libproc RSS/physical footprint，每 20 ms 检查 256 MiB 阈值并 kill/reap。此为运行期 watchdog，**不是内核硬内存上限**，检查间可能超出阈值；macOS RLIMIT_RSS 是压力下的回收偏好，不能当作严格堆限制。输出/诊断都有独立字节预算，取消必须 join 真实 reader、watchdog 与子进程。

## 约束
Processor 保留未完成物理 cleanup 的子进程/名额，Close 超时不伪报完成；原 owner 可重试 Close。其他平台显式 unsupported，不安装 parser 或自动执行用户应用。实际 OS 测试用生成的无动作 PDF fixture，不读取用户文件；跨平台编译不等于平台验收。

原生契约来源：[Apple PDFPage.string](https://developer.apple.com/documentation/pdfkit/pdfpage/string)、[PDFDocument.pageCount](https://developer.apple.com/documentation/pdfkit/pdfdocument/pagecount)、[JavaScriptObjC bridge](https://developer.apple.com/library/archive/documentation/LanguagesUtilities/Conceptual/MacAutomationScriptingGuide/HowMacScriptingWorks.html)。owned memory ABI 来自 macOS SDK libproc.h/sys/resource.h 的 proc_pid_rusage/RUSAGE_INFO_V0。

## 业务域清单
| 文件 | 职责 |
|---|---|
| document.go | 冷 Processor、输入/并发/输出预算、真实物理 owner 与 Close/cleanup |
| pdf_darwin.go | 固定 PDFKit/JXA 只读 fd worker、原文提取、内存/时限/双流预算与真实 join |
| pdf_other.go / pdf_other_test.go | 未验证平台明确 unsupported，不发现或获取替代工具 |
| document_test.go | 冷构造、未启动的输入上限与幂等真实 Close |
| pdf_darwin_test.go | 实际 OS PDFKit 文本/页上限、低阈值 watchdog kill/reap 与活跃 decoder 取消回归 |
