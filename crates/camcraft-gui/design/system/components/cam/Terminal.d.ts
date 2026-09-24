/** Monospace output block. lines: string | Array<[text, color?]> where color is pass|FAIL|dim|bold|accent or CSS.
 * @startingPoint section="CAM" subtitle="CLI output block" viewport="700x260" */
export interface TerminalProps { title?: React.ReactNode; lines?: Array<string | Array<[string, string?]>>; children?: React.ReactNode; height?: number|string; style?: React.CSSProperties; }
export function Terminal(props: TerminalProps): JSX.Element;