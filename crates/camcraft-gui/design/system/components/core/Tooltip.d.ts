/** Hover tooltip with optional mono shortcut. */
export interface TooltipProps { label: React.ReactNode; shortcut?: string; children: React.ReactNode; side?: 'top'|'bottom'; }
export function Tooltip(props: TooltipProps): JSX.Element;