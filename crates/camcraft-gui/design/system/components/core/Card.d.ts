/** Bordered surface with an 11px uppercase header. No shadow, 2px radius.
 * @startingPoint section="Layout" subtitle="Panel card with header" viewport="700x200" */
export interface CardProps { title?: React.ReactNode; actions?: React.ReactNode; padding?: number|string; children?: React.ReactNode; style?: React.CSSProperties; }
export function Card(props: CardProps): JSX.Element;