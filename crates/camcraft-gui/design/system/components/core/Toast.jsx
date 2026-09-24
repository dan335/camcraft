import React from 'react';
const C={ok:'var(--ok)',warn:'var(--warn)',err:'var(--err)',info:'var(--info)',neutral:'var(--text-3)'};
export function Toast({tone='neutral',title,detail,action,style}){
  return <div role="status" style={{display:'flex',gap:10,alignItems:'flex-start',width:360,padding:'10px 12px',background:'var(--glass)',backdropFilter:'blur(var(--blur-panel))',borderRadius:'var(--radius-md)',boxShadow:'var(--shadow-popover)',fontSize:13,...style}}>
    <span style={{width:6,height:6,background:C[tone],marginTop:6,flexShrink:0}}/>
    <div style={{flex:1,minWidth:0}}><div style={{color:'var(--text-1)',fontWeight:500}}>{title}</div>{detail&&<div style={{color:'var(--text-3)',fontFamily:'var(--font-mono)',fontSize:12,marginTop:2}}>{detail}</div>}</div>
    {action}
  </div>;
}