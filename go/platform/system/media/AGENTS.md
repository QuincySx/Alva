# 横向支撑 · Explicit native media processing
> 用户明确选择的字节生成有界派生结果，处理本身不向模型投递。

## 地位
native 能力，不进 kernel/agent SDK；调用方拥有 artifact 归属、Guard 和 durable intent。

## 逻辑
PNG/JPEG/GIF 使用标准库先查尺寸再解码首图，缩为最大512px白底 JPEG quality75。
视频/音频/其他受支持格式仅使用显式配置的 ffmpeg/ffprobe，固定协议/demuxer和私有只读文件能力；Unix fd3，Windows为只读seekable stdin/handle协议。
输入/产物哈希与真实处理参数由上层保存在同一 native artifact authority。

## 约束
最多20MiB输入、16MP/8192边、256KiB JPEG；不发现exe、下载程序、读取URL或原件路径。
不宣称语义理解、动画识别、色彩/元数据保真或隐私scrub；标准库解码有输入/像素界，
没有可强行中断的Go解码线程或OS CPU/RSS硬隔离，不把context检查冒充解码器沙箱。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| image.go | 标准库图片真实探测与固定有损JPEG派生、透明白底和来源损失说明 |
| image_test.go | 实际PNG/GIF/JPEG、边界/损坏/取消/透明背景与不改原件回归 |
| decoder.go | 显式owned ffprobe/ffmpeg、只读FD、实际音视频探测/PTS帧选择与静音限时转码 |
| input_other.go / input_windows.go / input_test.go / input_windows_test.go | Unix unlink与Windows原子CREATE_NEW/delete-on-close/只读duplicate；精确输入对象、seek/不可写/最后handle回收，Windows拒绝pathname或不seekable pipe回退 |
| private_other.go / private_windows.go | Unix mode与Windows DACL分开校验实际decoder临时根，不把权限位误作跨平台ACL |
| audio.go / audio_test.go | 显式首音轨≤60秒mono16kHz PCM派生、实际sample时长、canonical WAV严格验证与伪造/超限/取消反例；无播放设备 |
| lifetime.go / lifetime_test.go | terminal Processor、active-call join、pending真实process与全局child slots保留；超时非reap，Close可重试且取消等待预算 |
| decoder_test.go | 真实自生成带音视频、均匀/指定时间、实际PTS/静音codec/fps/取消及无残留快照 |
| player.go / player_test.go | 显式已验证silent MP4逐帧RGBA、单consumer背压、5fps名义播放clock、seek重建和Pause/Close真reap，不宣称持久receipt或原始PTS |

Windows需要显式decoder构建支持[FFmpeg handle protocol](https://ffmpeg.org/ffmpeg-protocols.html#handle)，缺失明确失败，不自动发现/下载或回退原件路径。input handle只有GENERIC_READ/SYNCHRONIZE，snapshot的write/delete owner留宿主，只有handle/pipe在Windows输入protocol whitelist中。Windows原生测试已写，交叉编译不是实机验收。
