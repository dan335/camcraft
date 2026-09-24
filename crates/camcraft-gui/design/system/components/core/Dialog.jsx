import React from 'react';
export function Dialog({open,title,children,footer,onClose,width=440}){
  if(!open)return null;
  return <div onClick={onClose} style={{position:'fixed',inset:0,background:'rgba(0,0,0,.6)',display:'flex',alignItems:'center',justifyContent:'center',zIndex:100}}>
    <div role="dialog" onClick={e=>e.stopPropagation()} style={{width,maxWidth:'calc(100% - 32px)',background:'var(--surface-2)',borderRadius:'var(--radius-lg)',boxShadow:'var(--shadow-dialog)'}}>
      <header style={{padding:'14px 16px',borderBottom:'1px solid var(--border)',fontSize:14,fontWeight:600}}>{title}</header>
      <div style={{padding:16,fontSize:13,color:'var(--text-2)'}}>{children}</div>
      {footer&&<footer style={{padding:'12px 16px',borderTop:'1px solid var(--border)',display:'flex',justifyContent:'flex-end',gap:8}}>{footer}</footer>}
    </div></div>;
}