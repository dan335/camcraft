import React from 'react';
export function Tooltip({label,shortcut,children,side='top'}){
  const [o,setO]=React.useState(false);
  const pos=side==='bottom'?{top:'calc(100% + 6px)'}:{bottom:'calc(100% + 6px)'};
  return <span style={{position:'relative',display:'inline-flex'}} onMouseEnter={()=>setO(true)} onMouseLeave={()=>setO(false)}>{children}
    {o&&<span role="tooltip" style={{position:'absolute',left:'50%',transform:'translateX(-50%)',...pos,whiteSpace:'nowrap',padding:'4px 8px',background:'var(--gray-4)',color:'var(--text-1)',fontSize:12,borderRadius:'var(--radius-sm)',boxShadow:'var(--shadow-popover)',display:'flex',gap:8,alignItems:'center',zIndex:50}}>{label}{shortcut&&<kbd style={{fontFamily:'var(--font-mono)',fontSize:11,color:'var(--text-3)'}}>{shortcut}</kbd>}</span>}
  </span>;
}