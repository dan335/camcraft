/** Status badge; tone maps to toolpath/status colours. dot adds a 6px square. */
export interface BadgeProps { tone?: 'neutral'|'ok'|'warn'|'err'|'info'|'accent'; dot?: boolean; mono?: boolean; children: React.ReactNode; }
export function Badge(props: BadgeProps): JSX.Element;