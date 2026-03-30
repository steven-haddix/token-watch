import DispatchSection from "./components/DispatchSection";
import { useClaudeUsage, useCodexUsage } from "./hooks/useUsage";

interface SchedulingViewProps {
  claudeUsage: ReturnType<typeof useClaudeUsage>;
  codexUsage: ReturnType<typeof useCodexUsage>;
}

export default function SchedulingView({ claudeUsage, codexUsage }: SchedulingViewProps) {
  return (
    <div className="flex flex-col gap-3">
      <DispatchSection claudeUsage={claudeUsage} codexUsage={codexUsage} />
    </div>
  );
}
