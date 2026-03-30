import { useState } from "react";
import { Button } from "@heroui/react";
import { useClaudeUsage, useCodexUsage } from "./hooks/useUsage";
import CompactView from "./CompactView";
import UsageView from "./UsageView";
import SchedulingView from "./SchedulingView";

function formatLastUpdated(date: Date): string {
	return date.toLocaleTimeString([], {
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
	});
}

export function mostRecentDate(...dates: Array<Date | null>): Date | null {
	const validDates = dates.filter((date): date is Date => date != null);
	if (validDates.length === 0) return null;
	return validDates.reduce((latest, current) =>
		current.getTime() > latest.getTime() ? current : latest,
	);
}

function App() {
	const isCompact =
		new URLSearchParams(window.location.search).get("compact") === "1";
	if (isCompact) return <CompactView />;
	return <FullView />;
}

type View = "usage" | "scheduling";

export function FullView() {
	const [activeView, setActiveView] = useState<View>("usage");
	const claudeUsage = useClaudeUsage();
	const codexUsage = useCodexUsage();
	const isMac = navigator.userAgent.includes("Mac");

	const lastUpdated = mostRecentDate(
		claudeUsage.lastUpdated,
		codexUsage.lastUpdated,
	);

	function handleRefresh() {
		claudeUsage.refresh();
		codexUsage.refresh();
	}

	return (
		<div
			className={`flex h-screen bg-background overflow-hidden ${isMac ? "pt-4" : ""}`}
		>
			{/* Sidebar */}
			<div className="w-16 md:w-48 flex flex-col border-r border-separator bg-content1/20 shrink-0">
				{/* Logo/Title */}
				<div
					data-tauri-drag-region
					className="h-14 flex items-center px-4 gap-3 select-none"
				>
					<div className="w-8 h-8 rounded-lg bg-gradient-to-br from-orange-400 to-violet-500 shrink-0" />
					<span className="hidden md:block font-bold text-foreground truncate">
						Token Watch
					</span>
				</div>

				<nav className="flex flex-col gap-1 px-2 pt-4 pb-2">
					<SidebarItem
						active={activeView === "usage"}
						onClick={() => setActiveView("usage")}
						label="Usage"
						icon={<UsageIcon />}
					/>
					<SidebarItem
						active={activeView === "scheduling"}
						onClick={() => setActiveView("scheduling")}
						label="Scheduling"
						icon={<SchedulingIcon />}
					/>
				</nav>

				{/* Sidebar Footer */}
				<div className="mt-auto p-4 flex flex-col gap-2">
					<div className="hidden md:flex flex-col gap-0.5">
						<span className="text-[10px] uppercase font-bold text-muted-foreground/50 tracking-wider">
							Last Sync
						</span>
						<span className="text-xs text-muted">
							{lastUpdated ? formatLastUpdated(lastUpdated) : "—"}
						</span>
					</div>
					<Button
						size="sm"
						variant="secondary"
						onPress={handleRefresh}
						isDisabled={claudeUsage.loading || codexUsage.loading}
					>
						<span className="hidden md:inline">Refresh</span>
						<RefreshIcon
							className={`w-4 h-4 md:ml-2 ${claudeUsage.loading || codexUsage.loading ? "animate-spin" : ""}`}
						/>
					</Button>
				</div>
			</div>

			{/* Main Content */}
			<div className="flex flex-col flex-1 overflow-hidden">
				{/* Header/Drag area */}
				<header
					data-tauri-drag-region
					className={`h-14 flex items-center justify-between bg-background select-none shrink-0 ${isMac ? "pl-8 pr-6" : "px-6"}`}
				>
					<h1 className="text-lg font-semibold text-foreground capitalize">
						{activeView}
					</h1>
					<div className="flex items-center gap-2">
						{/* Window controls could go here if custom */}
					</div>
				</header>

				<main className="flex-1 overflow-y-auto p-4 md:p-6 custom-scrollbar">
					<div className="max-w-4xl mx-auto">
						{activeView === "usage" && (
							<UsageView claudeUsage={claudeUsage} codexUsage={codexUsage} />
						)}
						{activeView === "scheduling" && (
							<SchedulingView
								claudeUsage={claudeUsage}
								codexUsage={codexUsage}
							/>
						)}
					</div>
				</main>
			</div>
		</div>
	);
}

interface SidebarItemProps {
	active: boolean;
	onClick: () => void;
	label: string;
	icon: React.ReactNode;
}

function SidebarItem({ active, onClick, label, icon }: SidebarItemProps) {
	return (
		<button
			onClick={onClick}
			className={`
        flex items-center gap-3 px-3 py-2 rounded-xl transition-all duration-200
        ${
					active
						? "bg-primary text-primary-foreground shadow-sm shadow-primary/20"
						: "text-muted hover:bg-content1/50 hover:text-foreground"
				}
      `}
		>
			<div className="shrink-0">{icon}</div>
			<span className="hidden md:block font-medium text-sm">{label}</span>
		</button>
	);
}

function UsageIcon() {
	return (
		<svg
			width="20"
			height="20"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M3 3v18h18" />
			<path d="M18 17V9" />
			<path d="M13 17V5" />
			<path d="M8 17v-3" />
		</svg>
	);
}

function SchedulingIcon() {
	return (
		<svg
			width="20"
			height="20"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<circle cx="12" cy="12" r="10" />
			<polyline points="12 6 12 12 16 14" />
		</svg>
	);
}

function RefreshIcon({ className }: { className?: string }) {
	return (
		<svg
			className={className}
			width="16"
			height="16"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
			<path d="M21 3v5h-5" />
		</svg>
	);
}

export default App;
