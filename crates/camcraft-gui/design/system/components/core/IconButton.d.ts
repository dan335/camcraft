/** Square icon-only button; wraps Button. */
export interface IconButtonProps { label: string; children: React.ReactNode; variant?: 'primary'|'secondary'|'ghost'|'danger'; size?: 'sm'|'md'|'lg'; disabled?: boolean; onClick?: () => void; }
export function IconButton(props: IconButtonProps): JSX.Element;