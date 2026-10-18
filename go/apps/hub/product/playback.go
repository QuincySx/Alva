// INPUT: Claimed branch, verified completed audio/video derivative and independently approved decoder snapshot
// OUTPUT: Explicit caller-owned video Playback or canonical audio bytes without starting playback
// POS: [L7 Hub product] UI playback reuses native artifact authority; no original upload, remote URL, model send or persistent processing receipt
package product

import (
	"context"
	"errors"
	"path/filepath"

	mediaextension "github.com/QuincySx/Alva/go/extension/media"
	private "github.com/QuincySx/Alva/go/platform/storage/sqlite"
	nativemedia "github.com/QuincySx/Alva/go/platform/system/media"
)

// MediaPlayback is an explicit native UI action. Construction reads the one
// selected verified derivative; Run is the separate caller-owned decode step.
// No HTTP endpoint accepts host executable/source paths through this method.
func (s *HubService) MediaPlayback(ctx context.Context, id, branchID, manifestURI string) (mediaextension.Manifest, *nativemedia.Playback, error) {
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	if !settings.MediaDecoder.Enabled || !settings.MediaDecoder.HostAccessApproved {
		return mediaextension.Manifest{}, nil, errors.New("local video playback requires separately approved explicit decoder paths")
	}
	manifest, data, err := s.MediaPreview(ctx, id, branchID, manifestURI)
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	if manifest.Artifact.MediaType != "video/mp4" || manifest.Output.Operation.Kind != "transcode_video" {
		return mediaextension.Manifest{}, nil, errors.New("play only completed silent transcode derivatives, not original media")
	}
	directory, err := private.EnsurePrivateDir(filepath.Join(s.DataDirectory, "media-playback"))
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	processor, err := nativemedia.NewProcessor(nativemedia.ProcessorConfig{FFmpeg: settings.MediaDecoder.FFmpeg, FFprobe: settings.MediaDecoder.FFprobe, TempDirectory: directory})
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	player, err := processor.NewPlayback(nativemedia.PlaybackInput{Data: data, SHA256: manifest.Artifact.SHA256, Width: manifest.Output.Width, Height: manifest.Output.Height, DurationMillis: manifest.Output.DurationMillis, Codec: manifest.Output.Codec, FrameRate: manifest.Output.FrameRate})
	return manifest, player, err
}

// AudioPlayback returns a hash-checked, canonical <=60s PCM derivative. Only
// the native UI owns its audio device voice; reading here starts no decoder,
// device, Agent or model. Uploaded originals never reach this surface.
func (s *HubService) AudioPlayback(ctx context.Context, id, branchID, manifestURI string) (mediaextension.Manifest, []byte, error) {
	manifest, data, err := s.MediaPreview(ctx, id, branchID, manifestURI)
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	if manifest.Artifact.MediaType != "audio/wav" || manifest.Output.Operation.Kind != "transcode_audio" {
		return mediaextension.Manifest{}, nil, errors.New("audio playback requires a completed canonical audio derivative")
	}
	return manifest, data, nil
}
