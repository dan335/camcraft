import React from 'react';
export function Card({title,actions,padding=16,children,style}){
  return <section style={{background:'var(--surface-2)',border:'1px solid var(--border)',borderRadius:'var(--radius-sm)',display:'flex',flexDirection:'column',...style}}>
    {(title||actions)&&<header style={{display:'flex',alignItems:'center',justifyContent:'space-between',height:32,padding:'0 12px 0 16px',borderBottom:'1px solid var(--border)',fontSize:11,fontWeight:500,letterSpacing:'.06em',textTransform:'uppercase',color:'var(--text-3)'}}><span>{title}</span><span style={{display:'flex',gap:4}}>{actions}</span></header>}
    <div style={{padding}}>{children}</div>
  </section>;
}