/** Primary/secondary/ghost/danger button. Sharp 2px radius, 24/28/36 heights.
 * @startingPoint section="Controls" subtitle="Button variants and sizes" viewport="700x150" */
export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'sm' | 'md' | 'lg';
  /** 16px Lucide icon, rendered before the label. Icon-only when children is empty. */
  icon?: React.ReactNode;
  disabled?: boolean;
  children?: React.ReactNode;
}
export function Button(props: ButtonProps): JSX.Element;