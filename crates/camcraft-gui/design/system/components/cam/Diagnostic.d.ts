/** Structured diagnostic: level, code, message, location. Never silence. */
export interface DiagnosticProps { level?: 'error'|'warn'|'info'|'ok'; code?: string; message: React.ReactNode; location?: React.ReactNode; children?: React.ReactNode; }
export function Diagnostic(props: DiagnosticProps): JSX.Element;