# Native local dictation
> Explicit microphone capture and operating-system local speech recognition.

## 地位
底层可选宿主原语；不依赖 Hub、MyGo、SDK 模型或数据库。

## 逻辑
Start才请求设备/识别权限；只接受系统本地识别，缺模型或平台明确失败，不退到网络。Stop请求最终文本，Cancel丢弃结果；Close等待native回调/音频资源真实收尾，超时保留owner。

## 约束
录音最多60秒，文字最多32KiB；不保存录音、不发送模型、不控制Agent任务。设备/语言能力和真实声学验收分别报告，fake驱动只能证明生命周期。

## 业务域清单
| 文件 | 职责 |
|---|---|
| speech.go | 显式开始、有限快照、独立Stop/Cancel与真实Close等待 |
| speech_darwin.go | macOS Speech/AVFoundation本地识别与音频回调owner |
| speech_other.go | 其他平台明确unsupported，无自动fallback |
| speech_test.go | 可控native驱动的取消/晚到文本/收尾与预算回归 |
| speech_darwin_test.go | 真实Foundation字符串/Block ABI冷检查，不请求设备许可 |
