interface PlaceholderProps {
  title: string;
  description?: string;
}

export default function Placeholder({ title, description }: PlaceholderProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center bg-neutral-950 text-neutral-300">
      <div className="text-lg font-semibold mb-2">{title}</div>
      <div className="text-sm text-neutral-500 max-w-md text-center px-6">
        {description ?? "这一页还没实现。先把壳架好,下一批填实。"}
      </div>
    </div>
  );
}
