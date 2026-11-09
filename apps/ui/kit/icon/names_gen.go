// INPUT: icons.json, the design interface icon manifest (Lucide 1.61.0 subset).
// OUTPUT: Name constants in design order and the Lucide file behind each. Generated: do not edit.
// POS: [L7 Kit] Single Go source of icon names; regenerate with go generate ./apps/ui/kit/icon.

package icon

// Design icon names (Alva Icon.Name axis). PanelLeft is the one derived name: Panel mirrored horizontally.
const (
	Plus         Name = "Plus"
	Close        Name = "Close"
	Search       Name = "Search"
	Check        Name = "Check"
	ChevronDown  Name = "ChevronDown"
	ChevronRight Name = "ChevronRight"
	ChevronLeft  Name = "ChevronLeft"
	ChevronUp    Name = "ChevronUp"
	ArrowUp      Name = "ArrowUp"
	ArrowRight   Name = "ArrowRight"
	Stop         Name = "Stop"
	Pause        Name = "Pause"
	Play         Name = "Play"
	More         Name = "More"
	Menu         Name = "Menu"
	Panel        Name = "Panel"
	Window       Name = "Window"
	Expand       Name = "Expand"
	External     Name = "External"
	Chat         Name = "Chat"
	Folder       Name = "Folder"
	File         Name = "File"
	Code         Name = "Code"
	Terminal     Name = "Terminal"
	Globe        Name = "Globe"
	Shield       Name = "Shield"
	Lock         Name = "Lock"
	Key          Name = "Key"
	Settings     Name = "Settings"
	Sliders      Name = "Sliders"
	User         Name = "User"
	Users        Name = "Users"
	Agent        Name = "Agent"
	Brain        Name = "Brain"
	Chip         Name = "Chip"
	Sparkles     Name = "Sparkles"
	Star         Name = "Star"
	Book         Name = "Book"
	Plugin       Name = "Plugin"
	Database     Name = "Database"
	Cloud        Name = "Cloud"
	Download     Name = "Download"
	Upload       Name = "Upload"
	Refresh      Name = "Refresh"
	Clock        Name = "Clock"
	Calendar     Name = "Calendar"
	Activity     Name = "Activity"
	Chart        Name = "Chart"
	List         Name = "List"
	Task         Name = "Task"
	Branch       Name = "Branch"
	Link         Name = "Link"
	Copy         Name = "Copy"
	Edit         Name = "Edit"
	Trash        Name = "Trash"
	Image        Name = "Image"
	Paperclip    Name = "Paperclip"
	Microphone   Name = "Microphone"
	Volume       Name = "Volume"
	Eye          Name = "Eye"
	Info         Name = "Info"
	Warning      Name = "Warning"
	Help         Name = "Help"
	Sun          Name = "Sun"
	Moon         Name = "Moon"
	Filter       Name = "Filter"
	Target       Name = "Target"
	Loader       Name = "Loader"
	PanelLeft    Name = "PanelLeft"
)

// lucideFile maps a design name to its file under lucide/ (without extension).
var lucideFile = map[Name]string{
	Plus:         "plus",
	Close:        "x",
	Search:       "search",
	Check:        "check",
	ChevronDown:  "chevron-down",
	ChevronRight: "chevron-right",
	ChevronLeft:  "chevron-left",
	ChevronUp:    "chevron-up",
	ArrowUp:      "arrow-up",
	ArrowRight:   "arrow-right",
	Stop:         "square",
	Pause:        "pause",
	Play:         "play",
	More:         "ellipsis",
	Menu:         "menu",
	Panel:        "panel-right",
	Window:       "panels-top-left",
	Expand:       "maximize",
	External:     "external-link",
	Chat:         "message-square",
	Folder:       "folder",
	File:         "file-text",
	Code:         "code-xml",
	Terminal:     "terminal",
	Globe:        "globe",
	Shield:       "shield-check",
	Lock:         "lock",
	Key:          "key-round",
	Settings:     "settings",
	Sliders:      "sliders-horizontal",
	User:         "user-round",
	Users:        "users-round",
	Agent:        "bot",
	Brain:        "brain",
	Chip:         "cpu",
	Sparkles:     "sparkles",
	Star:         "star",
	Book:         "book-open",
	Plugin:       "plug",
	Database:     "database",
	Cloud:        "cloud",
	Download:     "download",
	Upload:       "upload",
	Refresh:      "refresh-cw",
	Clock:        "clock",
	Calendar:     "calendar",
	Activity:     "activity",
	Chart:        "chart-column",
	List:         "list",
	Task:         "list-checks",
	Branch:       "git-branch",
	Link:         "link",
	Copy:         "copy",
	Edit:         "pencil",
	Trash:        "trash-2",
	Image:        "image",
	Paperclip:    "paperclip",
	Microphone:   "mic",
	Volume:       "volume-2",
	Eye:          "eye",
	Info:         "info",
	Warning:      "triangle-alert",
	Help:         "circle-question-mark",
	Sun:          "sun",
	Moon:         "moon",
	Filter:       "funnel",
	Target:       "target",
	Loader:       "loader-circle",
	PanelLeft:    "panel-right",
}

// designOrder lists the manifest names in design order, PanelLeft last.
var designOrder = []Name{
	Plus,
	Close,
	Search,
	Check,
	ChevronDown,
	ChevronRight,
	ChevronLeft,
	ChevronUp,
	ArrowUp,
	ArrowRight,
	Stop,
	Pause,
	Play,
	More,
	Menu,
	Panel,
	Window,
	Expand,
	External,
	Chat,
	Folder,
	File,
	Code,
	Terminal,
	Globe,
	Shield,
	Lock,
	Key,
	Settings,
	Sliders,
	User,
	Users,
	Agent,
	Brain,
	Chip,
	Sparkles,
	Star,
	Book,
	Plugin,
	Database,
	Cloud,
	Download,
	Upload,
	Refresh,
	Clock,
	Calendar,
	Activity,
	Chart,
	List,
	Task,
	Branch,
	Link,
	Copy,
	Edit,
	Trash,
	Image,
	Paperclip,
	Microphone,
	Volume,
	Eye,
	Info,
	Warning,
	Help,
	Sun,
	Moon,
	Filter,
	Target,
	Loader,
	PanelLeft,
}
