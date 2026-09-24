/** Native select styled as a hairline control with a Lucide chevron. */
export interface SelectProps extends React.SelectHTMLAttributes<HTMLSelectElement> { options: Array<string | {value: string; label: string}>; size?: 'sm'|'md'|'lg'; }
export function Select(props: SelectProps): JSX.Element;