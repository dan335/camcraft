/** Transient notification; glass panel with a square status dot. */
export interface ToastProps { tone?: 'ok'|'warn'|'err'|'info'|'neutral'; title: React.ReactNode; detail?: React.ReactNode; action?: React.ReactNode; style?: React.CSSProperties; }
export function Toast(props: ToastProps): JSX.Element;