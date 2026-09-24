Button — one amber primary per view; everything else secondary or ghost. Danger only for destructive actions (--no-verify style).
```jsx
<Button variant="primary">Emit G-code</Button>
<Button>Simulate</Button>
<Button variant="ghost" size="sm">Cancel</Button>
```
Props: variant primary|secondary|ghost|danger; size sm|md|lg; icon (16px node); disabled.