// INPUT: Explicit ffmpeg/ffprobe executables, bounded source bytes and private read-only descriptor snapshots
// OUTPUT: Processor audio/video probe, actual-timestamp frame extraction and bounded transcoding
// POS: [platform / OS primitive] Owned decoder subprocesses preserve processing evidence without ambient executable or source-path discovery
package media

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"image/jpeg"
	"io"
	"math"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type ProcessorConfig struct{ FFmpeg, FFprobe, TempDirectory string }
type Processor struct {
	config   ProcessorConfig
	mu       sync.Mutex
	closed   bool
	active   int
	idle     chan struct{}
	children map[*process.Process]func()
	lifetime context.Context
	stop     context.CancelFunc
}
type DecoderError struct {
	Log []byte
	Err error
}

func (*DecoderError) Error() string {
	return "media decoder did not complete; bounded diagnostic evidence is available to the owner"
}
func (e *DecoderError) Unwrap() error { return e.Err }

func NewProcessor(config ProcessorConfig) (*Processor, error) {
	for _, exe := range []string{config.FFmpeg, config.FFprobe} {
		if !filepath.IsAbs(exe) {
			return nil, errors.New("explicit absolute decoder executable paths required; no decoder was discovered or installed")
		}
		info, err := os.Stat(exe)
		if err != nil || !info.Mode().IsRegular() || (runtime.GOOS != "windows" && info.Mode().Perm()&0111 == 0) || (runtime.GOOS == "windows" && !strings.EqualFold(filepath.Ext(exe), ".exe")) {
			return nil, errors.New("explicit decoder executable is unavailable")
		}
	}
	canonical, err := filepath.EvalSymlinks(config.TempDirectory)
	if err != nil || !filepath.IsAbs(config.TempDirectory) {
		return nil, errors.New("explicit existing private decoder directory required")
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return nil, errors.New("decoder directory must be private to its owner")
	}
	if err := checkDecoderDirectory(canonical, info); err != nil {
		return nil, err
	}
	config.TempDirectory = canonical
	lifetime, stop := context.WithCancel(context.Background())
	return &Processor{config: config, lifetime: lifetime, stop: stop}, nil
}

func formatFor(mediaType string) (string, error) {
	switch mediaType {
	case "image/png", "image/jpeg", "image/webp":
		return "image2pipe", nil
	case "image/gif":
		return "gif", nil
	case "video/mp4", "video/quicktime":
		return "mov", nil
	case "video/webm":
		return "matroska", nil
	case "audio/wav", "audio/x-wav", "audio/wave":
		return "wav", nil
	case "audio/mpeg":
		return "mp3", nil
	case "audio/ogg", "application/ogg":
		return "ogg", nil
	case "audio/flac":
		return "flac", nil
	case "audio/mp4", "audio/x-m4a":
		return "mov", nil
	case "audio/webm":
		return "matroska", nil
	}
	return "", errors.New("unsupported explicit media type; source was not understood")
}

func (p *Processor) begin(ctx context.Context) (func(), error) {
	end, err := p.enter()
	if err != nil {
		return nil, err
	}
	wait, cancel := context.WithCancel(p.ownerContext())
	stop := context.AfterFunc(ctx, cancel)
	defer stop()
	defer cancel()
	if err = ctx.Err(); err != nil {
		end()
		return nil, err
	}
	release, err := beginProcessing(wait)
	if err != nil {
		end()
		return nil, err
	}
	return func() { end(); release() }, nil
}

// ProcessImage uses the standard-library path when available. WebP requires
// the separately configured native decoder; its quality/version is recorded
// as the actual ffmpeg operation, never mislabeled Go JPEG quality75.
func (p *Processor) ProcessImage(ctx context.Context, input []byte, mediaType string) (Output, error) {
	if mediaType != "image/webp" {
		end, err := p.enter()
		if err != nil {
			return Output{}, err
		}
		defer end()
		// Queueing for the shared standard-library budget is cancellable by
		// the actual owner too; only an already-entered codec lacks preemption.
		owned, cancel := context.WithCancel(p.ownerContext())
		stop := context.AfterFunc(ctx, cancel)
		defer stop()
		defer cancel()
		if err = ctx.Err(); err != nil {
			return Output{}, err
		}
		return ProcessImage(owned, input, mediaType)
	}
	end, err := p.begin(ctx)
	if err != nil {
		return Output{}, err
	}
	defer end()
	if _, err = p.probe(ctx, input, mediaType); err != nil {
		return Output{}, err
	}
	version, err := p.version(ctx)
	if err != nil {
		return Output{}, err
	}
	args, _ := decoderInput(mediaType)
	args = append([]string{"-nostdin", "-hide_banner", "-v", "error", "-threads", "1", "-filter_threads", "1"}, args...)
	args = append(args, "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", "scale=w='min(512,iw)':h='min(512,ih)':force_original_aspect_ratio=decrease", "-frames:v", "1", "-threads", "1", "-c:v", "mjpeg", "-q:v", "4", "-f", "image2pipe", "pipe:1")
	data, _, err := p.run(ctx, p.config.FFmpeg, args, input, MaxImageOutput)
	if err != nil {
		return Output{}, err
	}
	config, err := jpeg.DecodeConfig(bytes.NewReader(data))
	if err != nil || config.Width < 1 || config.Height < 1 || config.Width > 512 || config.Height > 512 {
		return Output{}, errors.New("WebP decoder did not return a valid bounded JPEG")
	}
	return Output{Data: data, MediaType: "image/jpeg", Width: config.Width, Height: config.Height, Version: version, Operation: wire.MediaOperation{Kind: "process_image", Description: "First decoded WebP image only, default scaler fit <=512px without crop/upscale, lossy MJPEG q=4; native decoder default alpha/colour behaviour, no model delivery.", InputSHA256: hash(input), OutputSHA256: hash(data), Parameters: []byte(`{"maximum_edge":512,"mjpeg_quality":4,"first_decoded_image_only":true,"crop":false,"alpha":"decoder default"}`)}, Losses: []string{"Lossy JPEG; first decoded image only, animation not detected or preserved.", "Decoder default scaling/alpha/colour/metadata, no fidelity or privacy-scrub guarantee; bytes may be larger than original."}}, nil
}

type captured struct {
	data   []byte
	err    error
	stream int
}

func (p *Processor) run(ctx context.Context, exe string, args []string, input []byte, limit int) ([]byte, []byte, error) {
	end, err := p.enter()
	if err != nil {
		return nil, nil, err
	}
	defer end()
	if len(input) > MaxInput || limit < 1 || limit > 8<<20 {
		return nil, nil, errors.New("decoder input/output budget exceeded")
	}
	ctx, cancel := context.WithTimeout(ctx, 35*time.Second)
	defer cancel()
	stopOwner := context.AfterFunc(p.ownerContext(), cancel)
	defer stopOwner()
	config := process.Config{Command: exe, Args: args, Cwd: p.config.TempDirectory}
	if input != nil {
		read, err := inputSnapshot(p.config.TempDirectory, "alva-media-", input)
		if err != nil {
			return nil, nil, err
		}
		defer read.Close()
		bindDecoderInput(&config, read)
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	child, err := p.startProcess(ctx, config)
	if err != nil {
		return nil, nil, err
	}
	_ = child.Stdin.Close()
	done := make(chan captured, 2)
	read := func(reader io.Reader, stream, budget int) {
		data, e := io.ReadAll(io.LimitReader(reader, int64(budget)+1))
		if len(data) > budget {
			data = data[:budget]
			e = errors.Join(e, errors.New("decoder output limit exceeded"))
		}
		done <- captured{data, e, stream}
	}
	go read(child.Stdout, 0, limit)
	go read(child.Stderr, 1, 128<<10)
	var stdout, stderr []byte
	var runErr error
	closing := false
	stop := func() {
		if !closing {
			closing = true
			child.BeginClose()
		}
	}
	for remaining := 2; remaining > 0; {
		select {
		case item := <-done:
			remaining--
			if item.stream == 0 {
				stdout = item.data
			} else {
				stderr = item.data
			}
			if item.err != nil && !(closing && errors.Is(item.err, os.ErrClosed)) {
				runErr = errors.Join(runErr, item.err)
				stop()
			}
		case <-ctx.Done():
			runErr = errors.Join(runErr, ctx.Err())
			stop()
			// Join both actual readers after owned pipe closure, not detached
			// work that outlives an apparent cancellation success.
			for remaining > 0 {
				item := <-done
				remaining--
				if item.stream == 0 {
					stdout = item.data
				} else {
					stderr = item.data
				}
				if item.err != nil && !errors.Is(item.err, os.ErrClosed) {
					runErr = errors.Join(runErr, item.err)
				}
			}
		}
	}
	if !closing {
		runErr = errors.Join(runErr, child.Wait(ctx))
	}
	cleanup, cleanupCancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cleanupCancel()
	runErr = errors.Join(runErr, child.Close(cleanup))
	p.releaseChild(child)
	code, exited := child.ExitCode()
	if !exited || code != 0 || !child.CleanupComplete() {
		runErr = errors.Join(runErr, errors.New("decoder exit or cleanup was not successful"))
	}
	if runErr != nil {
		return nil, stderr, &DecoderError{Log: append([]byte(nil), stderr...), Err: runErr}
	}
	return stdout, stderr, nil
}

func decoderInput(mediaType string) ([]string, error) {
	format, err := formatFor(mediaType)
	if err != nil {
		return nil, err
	}
	args := []string{"-max_alloc", "67108864", "-probesize", "1048576", "-analyzeduration", "5000000", "-protocol_whitelist", decoderProtocols, "-f", format}
	if format == "mov" {
		args = append(args, "-enable_drefs", "0", "-use_absolute_path", "0")
	}
	return append(args, "-i", decoderInputURL), nil
}

func (p *Processor) probe(ctx context.Context, input []byte, mediaType string) (Probe, error) {
	if len(input) == 0 || len(input) > MaxInput {
		return Probe{}, errors.New("media requires 1..20 MiB original bytes")
	}
	args, err := decoderInput(mediaType)
	if err != nil {
		return Probe{}, err
	}
	args = append([]string{"-v", "error"}, args...)
	args = append(args, "-show_entries", "stream=width,height,codec_type,codec_name,r_frame_rate,duration,sample_rate,channels:format=duration", "-of", "json")
	data, _, err := p.run(ctx, p.config.FFprobe, args, input, 64<<10)
	if err != nil {
		return Probe{}, err
	}
	var result struct {
		Streams []struct {
			Width, Height int
			Codec         string `json:"codec_name"`
			Type          string `json:"codec_type"`
			FPS           string `json:"r_frame_rate"`
			Duration      string `json:"duration"`
			SampleRate    string `json:"sample_rate"`
			Channels      int    `json:"channels"`
		}
		Format struct {
			Duration string `json:"duration"`
		}
	}
	if !wire.ValidJSON(data) || json.Unmarshal(data, &result) != nil || len(result.Streams) == 0 || len(result.Streams) > 64 {
		return Probe{}, errors.New("decoder returned invalid bounded stream metadata")
	}
	videoIndex := -1
	audioIndex := -1
	audioCount := 0
	for i, stream := range result.Streams {
		if stream.Type == "video" && videoIndex < 0 {
			videoIndex = i
		}
		if stream.Type == "audio" {
			audioCount++
			if audioIndex < 0 {
				audioIndex = i
			}
		}
	}
	audio := strings.HasPrefix(mediaType, "audio/") || mediaType == "application/ogg"
	index := videoIndex
	if audio {
		index = audioIndex
	}
	if index < 0 {
		return Probe{}, errors.New("source lacks the selected declared media stream")
	}
	stream := result.Streams[index]
	sampleRate := 0
	if audio {
		sampleRate, err = strconv.Atoi(stream.SampleRate)
		if err != nil || sampleRate < 1 || sampleRate > 384000 || stream.Channels < 1 || stream.Channels > 64 {
			return Probe{}, errors.New("invalid bounded audio stream metadata")
		}
		stream.Width, stream.Height, stream.FPS = 0, 0, ""
	} else if err = checkDimensions(stream.Width, stream.Height); err != nil {
		return Probe{}, err
	}
	duration := stream.Duration
	if duration == "" || duration == "N/A" {
		duration = result.Format.Duration
	}
	millis := int64(0)
	if duration != "" && duration != "N/A" {
		seconds, e := strconv.ParseFloat(duration, 64)
		if e != nil || math.IsNaN(seconds) || math.IsInf(seconds, 0) || seconds < 0 || seconds > 3600 {
			return Probe{}, errors.New("source duration exceeds bounded 3600 seconds or is invalid")
		}
		millis = int64(math.Round(seconds * 1000))
	}
	if (audio || strings.HasPrefix(mediaType, "video/")) && millis <= 0 {
		return Probe{}, errors.New("audio/video requires a known positive duration")
	}
	selected := "video"
	if audio {
		selected = "audio"
	}
	return Probe{MediaType: mediaType, Width: stream.Width, Height: stream.Height, DurationMillis: millis, Codec: stream.Codec, FrameRate: stream.FPS, AudioStreams: audioCount, SampleRate: sampleRate, Channels: stream.Channels, Processing: "ffprobe metadata only; first " + selected + " stream selected. No semantic understanding. Duration is rounded to milliseconds. Native decoder is not an OS sandbox."}, nil
}

func (p *Processor) Probe(ctx context.Context, input []byte, mediaType string) (Probe, error) {
	end, err := p.begin(ctx)
	if err != nil {
		return Probe{}, err
	}
	defer end()
	return p.probe(ctx, input, mediaType)
}

func (p *Processor) version(ctx context.Context) (string, error) {
	data, _, err := p.run(ctx, p.config.FFmpeg, []string{"-version"}, nil, 64<<10)
	if err != nil {
		return "", err
	}
	line, _, _ := strings.Cut(string(data), "\n")
	if len(line) > 256 {
		return "", errors.New("decoder version exceeds provenance limit")
	}
	return line, nil
}

// Only the first frame record of our fixed select,showinfo,scale filter graph
// is evidence. Input titles/tags and arbitrary diagnostics may contain the
// same words; matching an unqualified pts_time anywhere forges provenance.
var framePTS = regexp.MustCompile(`(?m)^\[Parsed_showinfo_1 @ 0x[0-9a-fA-F]+\]\s+n:\s*0\s+pts:\s*[-+0-9]+\s+pts_time:([-+0-9.eE]+)\s`)

// ExtractFrames chooses explicit millisecond positions or uniform interior
// positions duration*i/(n+1); it never claims scene detection or full-video input.
func (p *Processor) ExtractFrames(ctx context.Context, input []byte, mediaType string, timestamps []int64, count int, reason string) ([]Output, error) {
	if len(reason) == 0 || len(reason) > 200 || ((len(timestamps) > 0) == (count > 0)) || len(timestamps) > 4 || count > 4 {
		return nil, errors.New("choose 1..4 timestamps OR a uniform frame count and an explicit bounded reason")
	}
	end, err := p.begin(ctx)
	if err != nil {
		return nil, err
	}
	defer end()
	probe, err := p.probe(ctx, input, mediaType)
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(mediaType, "video/") {
		return nil, errors.New("frame extraction requires an uploaded video")
	}
	strategy := "explicit"
	if count > 0 {
		strategy = "uniform interior"
		timestamps = make([]int64, count)
		for i := range timestamps {
			timestamps[i] = probe.DurationMillis * int64(i+1) / int64(count+1)
		}
	} else {
		timestamps = append([]int64(nil), timestamps...)
	}
	for i, t := range timestamps {
		if t < 0 || t >= probe.DurationMillis || (i > 0 && t <= timestamps[i-1]) {
			return nil, errors.New("frame timestamps must be strictly increasing within source duration")
		}
	}
	version, err := p.version(ctx)
	if err != nil {
		return nil, err
	}
	var outputs []Output
	for _, requested := range timestamps {
		args, _ := decoderInput(mediaType)
		args = append([]string{"-nostdin", "-hide_banner", "-v", "info", "-threads", "1", "-filter_threads", "1"}, args...)
		filter := fmt.Sprintf("select=gte(t\\,%.3f),showinfo,scale=w='min(512,iw)':h='min(512,ih)':force_original_aspect_ratio=decrease", float64(requested)/1000)
		args = append(args, "-map", "0:v:0", "-an", "-sn", "-dn", "-vf", filter, "-frames:v", "1", "-threads", "1", "-c:v", "mjpeg", "-q:v", "4", "-f", "image2pipe", "pipe:1")
		data, logs, e := p.run(ctx, p.config.FFmpeg, args, input, MaxImageOutput)
		if e != nil {
			return nil, e
		}
		config, e := jpeg.DecodeConfig(bytes.NewReader(data))
		if e != nil || config.Width > 512 || config.Height > 512 {
			return nil, errors.New("frame decoder did not return a valid <=512px JPEG")
		}
		match := framePTS.FindSubmatch(logs)
		if len(match) != 2 {
			return nil, errors.New("decoder did not report the actual selected frame timestamp")
		}
		seconds, e := strconv.ParseFloat(string(match[1]), 64)
		if e != nil || math.IsNaN(seconds) || math.IsInf(seconds, 0) || seconds < 0 || seconds > 3600 {
			return nil, errors.New("invalid actual frame timestamp")
		}
		actual := int64(math.Round(seconds * 1000))
		if actual < requested || actual > probe.DurationMillis {
			return nil, errors.New("decoder frame was outside the requested/source range")
		}
		parameters, _ := json.Marshal(map[string]any{"strategy": strategy, "requested_millis": requested, "actual_pts_time": string(match[1]), "actual_millis_rounded": actual, "reason": reason, "maximum_edge": 512, "mjpeg_quality": 4})
		outputs = append(outputs, Output{Data: data, MediaType: "image/jpeg", Width: config.Width, Height: config.Height, Version: version, RequestedMillis: &requested, ActualMillis: &actual, Operation: wire.MediaOperation{Kind: "extract_frame", Description: "First decoded frame at/after requested time, on ffmpeg normalized timeline; not container absolute time. Actual PTS retained, milliseconds rounded. Fit <=512px, MJPEG q=4, no cropping; no model delivery.", InputSHA256: hash(input), OutputSHA256: hash(data), FrameTimesMillis: []int64{actual}, Parameters: parameters}, Losses: []string{"Only selected individual frames; no scene detection, audio or full-video understanding.", "Lossy JPEG, default scaler/colour/metadata handling; no colour fidelity or privacy-scrub guarantee."}})
	}
	return outputs, nil
}

func (p *Processor) TranscodeVideo(ctx context.Context, input []byte, mediaType string) (Output, error) {
	end, err := p.begin(ctx)
	if err != nil {
		return Output{}, err
	}
	defer end()
	probe, err := p.probe(ctx, input, mediaType)
	if err != nil {
		return Output{}, err
	}
	if !strings.HasPrefix(mediaType, "video/") {
		return Output{}, errors.New("video transcode requires uploaded video")
	}
	version, err := p.version(ctx)
	if err != nil {
		return Output{}, err
	}
	args, _ := decoderInput(mediaType)
	args = append([]string{"-nostdin", "-hide_banner", "-v", "error", "-threads", "1", "-filter_threads", "1"}, args...)
	args = append(args, "-map", "0:v:0", "-t", "60", "-an", "-sn", "-dn", "-vf", "scale=w='min(720,iw)':h='min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,fps=5", "-c:v", "libx264", "-threads", "1", "-b:v", "400k", "-pix_fmt", "yuv420p", "-movflags", "frag_keyframe+empty_moov", "-f", "mp4", "pipe:1")
	data, _, err := p.run(ctx, p.config.FFmpeg, args, input, 8<<20)
	if err != nil {
		return Output{}, err
	}
	actual, err := p.probe(ctx, data, "video/mp4")
	if err != nil {
		return Output{}, err
	}
	if actual.Width > 720 || actual.Height > 720 || actual.DurationMillis > 60200 || actual.Codec != "h264" || actual.FrameRate != "5/1" || actual.AudioStreams != 0 {
		return Output{}, errors.New("video derivative failed actual codec/dimension/duration/frame-rate verification")
	}
	parameters, _ := json.Marshal(map[string]any{"source_duration_millis": probe.DurationMillis, "output_duration_millis": actual.DurationMillis, "range_start_millis": 0, "maximum_duration_millis": 60000, "maximum_edge": 720, "fps": 5, "target_bitrate": 400000, "audio_removed": true, "codec": actual.Codec, "actual_frame_rate": actual.FrameRate})
	return Output{Data: data, MediaType: "video/mp4", Width: actual.Width, Height: actual.Height, Version: version, DurationMillis: actual.DurationMillis, Codec: actual.Codec, FrameRate: actual.FrameRate, Operation: wire.MediaOperation{Kind: "transcode_video", Description: "First <=60 seconds only, <=720px without cropping, 5fps with possible dropped/duplicated frames, H.264 target400k, all audio/subtitles/data removed. Output probed; file only, not model video input.", InputSHA256: hash(input), OutputSHA256: hash(data), Parameters: parameters}, Losses: []string{"Lossy re-encoding, temporal resampling and possible range truncation.", "All audio/subtitles/data removed. Default colour/metadata handling, no fidelity or privacy-scrub guarantee."}}, nil
}
