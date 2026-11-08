// INPUT: One settings section of hub.Capabilities
// OUTPUT: Chinese per-field FieldErrors for form correction before the shared hub validation runs
// POS: [L7 Hub product] Form guidance only; hub.Settings.Validate stays the authority at rest
package product

import (
	"fmt"
	"net/http"
	"net/url"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

func plainText(value string, max int) bool {
	return len(value) <= max && utf8.ValidString(value) && !strings.ContainsAny(value, "\x00\r\n")
}

func identity(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, r := range value {
		if !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || strings.ContainsRune("._:-", r)) {
			return false
		}
	}
	return true
}

func absolutePath(value string) bool {
	return filepath.IsAbs(value) && plainText(value, 4096)
}

func checkEnvironment(problems *FieldErrors, field string, values map[string]string, forbidden ...string) {
	if len(values) > 64 {
		*problems = append(*problems, FieldError{field, "环境变量最多 64 个。"})
	}
	for key, value := range values {
		if key == "" || len(key) > 128 || strings.ContainsAny(key, "= \x00") || !utf8.ValidString(key) {
			*problems = append(*problems, FieldError{field, fmt.Sprintf("环境变量名“%s”无效：不能为空，也不能包含空格或等号。", key)})
			continue
		}
		for _, name := range forbidden {
			if key == name {
				*problems = append(*problems, FieldError{field, fmt.Sprintf("不能覆盖环境变量 %s。", key)})
			}
		}
		if value != SecretKept && (len(value) > 8192 || strings.ContainsRune(value, 0) || !utf8.ValidString(value)) {
			*problems = append(*problems, FieldError{field, fmt.Sprintf("环境变量 %s 的值无效或超过 8 KiB。", key)})
		}
	}
}

func checkArgs(problems *FieldErrors, field string, args []string) {
	if len(args) > 128 {
		*problems = append(*problems, FieldError{field, "参数最多 128 个。"})
	}
	for _, arg := range args {
		if strings.ContainsRune(arg, 0) || !utf8.ValidString(arg) {
			*problems = append(*problems, FieldError{field, "参数包含无效字符。"})
		}
	}
}

func checkTimeout(problems *FieldErrors, field string, value, max int) {
	if value < 0 || value > max {
		*problems = append(*problems, FieldError{field, fmt.Sprintf("超时须在 0–%d 毫秒之间，0 表示默认值。", max)})
	}
}

func checkHTTPURL(raw string) (*url.URL, string) {
	address, err := url.Parse(raw)
	switch {
	case raw == "":
		return nil, "请填写地址。"
	case err != nil || !plainText(raw, 2048) || strings.ContainsAny(raw, " \t"):
		return nil, "地址格式无效。"
	case address.Scheme != "http" && address.Scheme != "https":
		return nil, "地址必须以 http:// 或 https:// 开头。"
	case address.Host == "":
		return nil, "地址缺少主机名。"
	case address.User != nil:
		return nil, "地址里不能包含用户名或密码，请改用请求头。"
	case address.Fragment != "":
		return nil, "地址不能包含 # 片段。"
	}
	return address, ""
}

// fieldErrors returns Chinese guidance for the fields of one section.
func fieldErrors(c hub.Capabilities, section string) FieldErrors {
	var problems FieldErrors
	switch section {
	case SectionMCP:
		if len(c.MCP) > 32 {
			problems = append(problems, FieldError{"mcp", "MCP 服务器最多 32 个。"})
		}
		seen := map[string]bool{}
		for _, server := range c.MCP {
			field := "mcp." + server.ID
			if !identity(server.ID) {
				problems = append(problems, FieldError{field + ".id", "标识只能包含字母、数字和 . _ : -，最长 128 个字符。"})
			} else if seen[server.ID] {
				problems = append(problems, FieldError{field + ".id", "标识与其他服务器重复。"})
			}
			seen[server.ID] = true
			if !plainText(server.Name, 120) {
				problems = append(problems, FieldError{field + ".name", "名称最长 120 个字符，且不能换行。"})
			}
			if server.Remote() {
				address, problem := checkHTTPURL(server.URL)
				if problem != "" {
					problems = append(problems, FieldError{field + ".url", problem})
				}
				if server.Auth == hub.MCPAuthOAuth && address != nil && address.Scheme != "https" && !hub.LoopbackHost(address.Hostname()) {
					problems = append(problems, FieldError{field + ".auth", "OAuth 授权要求 https 地址（本机回环地址除外），以免令牌明文传输。"})
				}
				if !plainText(server.OAuthClientID, 256) || strings.ContainsAny(server.OAuthClientID, " \t") {
					problems = append(problems, FieldError{field + ".oauth_client_id", "Client ID 无效。"})
				}
				if len(server.Headers) > 32 {
					problems = append(problems, FieldError{field + ".headers", "请求头最多 32 个。"})
				}
				for name, value := range server.Headers {
					canonical := http.CanonicalHeaderKey(name)
					switch {
					case name == "" || strings.ContainsAny(name, " :\r\n\x00"):
						problems = append(problems, FieldError{field + ".headers", fmt.Sprintf("请求头名称“%s”无效。", name)})
					case canonical == "Authorization" && server.Auth == hub.MCPAuthOAuth:
						problems = append(problems, FieldError{field + ".headers", "使用 OAuth 时不能再手动设置 Authorization 请求头。"})
					case canonical == "Host" || canonical == "Content-Type" || canonical == "Accept" || canonical == "Content-Length" || canonical == "Mcp-Session-Id" || canonical == "Mcp-Protocol-Version":
						problems = append(problems, FieldError{field + ".headers", fmt.Sprintf("请求头 %s 由传输层管理，不能手动设置。", canonical)})
					case value != SecretKept && (strings.ContainsAny(value, "\r\n\x00") || len(value) > 8192):
						problems = append(problems, FieldError{field + ".headers", fmt.Sprintf("请求头 %s 的值无效。", name)})
					}
				}
				continue
			}
			switch {
			case server.Command == "":
				problems = append(problems, FieldError{field + ".command", "请填写要运行的命令。"})
			case !plainText(server.Command, 4096) || !filepath.IsAbs(server.Command) && strings.ContainsAny(server.Command, "/\\"):
				problems = append(problems, FieldError{field + ".command", "命令须是绝对路径，或不含斜杠的程序名。"})
			}
			checkArgs(&problems, field+".args", server.Args)
			checkEnvironment(&problems, field+".env", server.Env)
		}
	case SectionAEP:
		if len(c.AEP) > 8 {
			problems = append(problems, FieldError{"aep", "扩展插件最多 8 个。"})
		}
		seen := map[string]bool{}
		for _, entry := range c.AEP {
			field := "aep." + entry.ID
			if !identity(entry.ID) || seen[entry.ID] {
				problems = append(problems, FieldError{field + ".id", "标识须唯一，只能包含字母、数字和 . _ : -。"})
			}
			seen[entry.ID] = true
			if !absolutePath(entry.SourceDirectory) {
				problems = append(problems, FieldError{field + ".source", "请选择插件源目录（绝对路径）。"})
			}
			if entry.Interpreter != "" && !absolutePath(entry.Interpreter) {
				problems = append(problems, FieldError{field + ".interpreter", "解释器须是绝对路径；WASM 插件留空。"})
			}
			checkArgs(&problems, field+".args", entry.InterpreterArgs)
			checkEnvironment(&problems, field+".env", entry.Env)
		}
	case SectionTools:
		shell := c.Tools.Shell
		if shell.Executable != "" || len(shell.Env) != 0 {
			if !absolutePath(shell.Executable) {
				problems = append(problems, FieldError{"tools.shell.executable", "Shell 可执行文件须是绝对路径。"})
			}
			checkEnvironment(&problems, "tools.shell.env", shell.Env)
		}
		web := c.Tools.Web
		if web.SearchEndpoint != "" {
			if address, problem := checkHTTPURL(web.SearchEndpoint); problem != "" {
				problems = append(problems, FieldError{"tools.web.search", problem})
			} else if address.RawQuery != "" {
				problems = append(problems, FieldError{"tools.web.search", "搜索端点不能带查询参数。"})
			}
		}
		if len(web.AllowedPrivateOrigins) > 16 {
			problems = append(problems, FieldError{"tools.web.origins", "内网来源最多 16 个。"})
		}
		for _, origin := range web.AllowedPrivateOrigins {
			address, problem := checkHTTPURL(origin)
			if problem == "" && (address.Path != "" || address.RawQuery != "" || address.Scheme+"://"+address.Host != origin) {
				problem = "只填协议、主机和端口，例如 http://192.168.1.10:8080。"
			}
			if problem != "" {
				problems = append(problems, FieldError{"tools.web.origins", origin + "：" + problem})
			}
		}
		checkTimeout(&problems, "tools.web.timeout", web.TimeoutMS, 60000)
	case SectionCommands:
		if len(c.Commands.Hooks) > 32 || len(c.Commands.Diagnostics) > 16 {
			problems = append(problems, FieldError{"commands", "Hooks 最多 32 个，诊断最多 16 个。"})
		}
		seen := map[string]bool{}
		for _, hook := range c.Commands.Hooks {
			field := "hook." + hook.ID
			if !identity(hook.ID) || seen[hook.ID] {
				problems = append(problems, FieldError{field + ".id", "标识须唯一，只能包含字母、数字和 . _ : -。"})
			}
			seen[hook.ID] = true
			switch hook.Event {
			case "PreToolUse", "PostToolUse", "PostToolUseFailure", "SessionStart", "SessionEnd", "Stop", "UserPromptSubmit", "Notification":
			default:
				problems = append(problems, FieldError{field + ".event", "请选择触发事件。"})
			}
			if !plainText(hook.Matcher, 256) || strings.Contains(strings.Trim(hook.Matcher, "*"), "*") {
				problems = append(problems, FieldError{field + ".matcher", "匹配规则只允许在首尾使用 *。"})
			}
			if !absolutePath(hook.Command) {
				problems = append(problems, FieldError{field + ".command", "命令须是绝对路径。"})
			}
			checkArgs(&problems, field+".args", hook.Args)
			checkEnvironment(&problems, field+".env", hook.Env)
			checkTimeout(&problems, field+".timeout", hook.TimeoutMS, 60000)
		}
		seen = map[string]bool{}
		for _, profile := range c.Commands.Diagnostics {
			field := "diagnostic." + profile.ID
			if !identity(profile.ID) || seen[profile.ID] {
				problems = append(problems, FieldError{field + ".id", "标识须唯一，只能包含字母、数字和 . _ : -。"})
			}
			seen[profile.ID] = true
			if len(profile.Description) > 512 || !utf8.ValidString(profile.Description) {
				problems = append(problems, FieldError{field + ".description", "说明最长 512 字节。"})
			}
			if !absolutePath(profile.Command) {
				problems = append(problems, FieldError{field + ".command", "命令须是绝对路径。"})
			}
			checkArgs(&problems, field+".args", profile.Args)
			checkEnvironment(&problems, field+".env", profile.Env)
			checkTimeout(&problems, field+".timeout", profile.TimeoutMS, 300000)
		}
	case SectionBrowser:
		b := c.Browser
		if b.Executable == "" && len(b.Env) == 0 && b.Proxy == "" && b.StartupTimeoutMS == 0 && b.OperationTimeoutMS == 0 {
			break
		}
		if !absolutePath(b.Executable) {
			problems = append(problems, FieldError{"browser.executable", "请填写浏览器可执行文件的绝对路径。"})
		}
		checkEnvironment(&problems, "browser.env", b.Env, "HOME", "USERPROFILE", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR", "CHROME_USER_DATA_DIR")
		if b.Proxy != "" {
			proxy, err := url.Parse(b.Proxy)
			if err != nil || proxy.User != nil || proxy.Host == "" || proxy.Path != "" || proxy.RawQuery != "" || proxy.Fragment != "" || proxy.Scheme != "http" && proxy.Scheme != "https" && proxy.Scheme != "socks5" {
				problems = append(problems, FieldError{"browser.proxy", "代理形如 http://host:port 或 socks5://host:port，不能包含账号、路径或参数。"})
			}
		}
		checkTimeout(&problems, "browser.startup_timeout", b.StartupTimeoutMS, 60000)
		checkTimeout(&problems, "browser.operation_timeout", b.OperationTimeoutMS, 60000)
	case SectionMedia:
		d := c.MediaDecoder
		if d.FFmpeg == "" && d.FFprobe == "" {
			break
		}
		if !absolutePath(d.FFmpeg) {
			problems = append(problems, FieldError{"media.ffmpeg", "请填写 ffmpeg 的绝对路径。"})
		}
		if !absolutePath(d.FFprobe) {
			problems = append(problems, FieldError{"media.ffprobe", "请填写 ffprobe 的绝对路径。"})
		}
	}
	return problems
}
