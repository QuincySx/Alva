// INPUT: Application-owned fixed Pi extension source and private native state
// OUTPUT: Content-addressed pre-execution approval and free-text question bridge
// POS: [integration / native external-engine adapter] Explicit extension injection never enables global extension discovery
package process

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"

	private "github.com/QuincySx/Alva/platform/system/privatefs"
)

const piBridgeSource = `import { Type } from "@earendil-works/pi-ai";
export default function(pi) {
  const nonce = process.env.ALVA_PI_BRIDGE_NONCE;
  let ready = false;
  pi.on("session_start", async (_event, ctx) => {
    if (ctx.mode !== "rpc" || !nonce) throw new Error("Alva bridge requires owned RPC mode");
    ready = true;
    ctx.ui.notify(JSON.stringify({alvaBridge:1,nonce,kind:"ready"}), "info");
  });
  pi.on("tool_call", async (event, ctx) => {
    if (!ready || ctx.mode !== "rpc") return {block:true,reason:"Alva approval bridge is not ready"};
    if (["read","grep","find","ls","ask_user"].includes(event.toolName)) return;
    if (!["bash","edit","write"].includes(event.toolName)) return {block:true,reason:"Tool is outside the explicit Alva native tool set"};
    const message = JSON.stringify({alvaBridge:1,nonce,kind:"permission",toolCallId:event.toolCallId,toolName:event.toolName,input:event.input});
    if (message.length > 1048576) return {block:true,reason:"Native tool approval input exceeds its bounded bridge"};
    const allowed = await ctx.ui.confirm("Alva native tool approval", message);
    if (!allowed) return {block:true,reason:"Native tool execution was declined by the user"};
  });
  pi.registerTool({
    name:"ask_user", label:"Ask user", description:"Ask the user one free-text question and wait for their answer.",
    parameters:Type.Object({question:Type.String({minLength:1,maxLength:4096})}),
    async execute(toolCallId, params, signal, _onUpdate, ctx) {
      if (!ready || ctx.mode !== "rpc") throw new Error("Alva question bridge is not ready");
      const title = JSON.stringify({alvaBridge:1,nonce,kind:"question",toolCallId,question:params.question});
      const value = await ctx.ui.input(title, "Your answer", {signal});
      return {content:[{type:"text",text:value === undefined ? "User skipped the question." : value}],details:{skipped:value === undefined}};
    }
  });
}
`

func (a *Adapter) piBridge() (string, error) {
	directory, err := private.EnsureDir(filepath.Join(a.config.StateDirectory, "bridges"))
	if err != nil {
		return "", err
	}
	digest := sha256.Sum256([]byte(piBridgeSource))
	path := filepath.Join(directory, "alva-pi-"+hex.EncodeToString(digest[:])+".mjs")
	file, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err == nil {
		_, writeErr := file.WriteString(piBridgeSource)
		syncErr := file.Sync()
		return path, errors.Join(writeErr, syncErr, file.Close())
	}
	if !errors.Is(err, os.ErrExist) {
		return "", err
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Size() != int64(len(piBridgeSource)) {
		return "", errors.New("owned Pi bridge source is unavailable or changed")
	}
	if err := checkNativePrivateFile(path, info); err != nil {
		return "", err
	}
	file, err = os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return "", errors.New("owned Pi bridge source changed while opening")
	}
	data, readErr := io.ReadAll(io.LimitReader(file, int64(len(piBridgeSource))+1))
	if readErr != nil || string(data) != piBridgeSource {
		return "", errors.New("owned Pi bridge source hash differs from the application source")
	}
	return path, nil
}
