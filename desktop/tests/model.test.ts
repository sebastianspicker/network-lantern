import { describe, it, expect } from 'vitest';
import { active, errorMessage, escape, fingerprint, requestFor, type Inputs } from '../src/model';
const input: Inputs = {host:'localhost',family:'IPv4',round:'Standard',engine:'path_basic',traceType:'ICMP',skip:true,target:'localhost',port:5201,protocol:'TCP',maxTests:0,action:'Verify',profile:'Safe',udpPort:5201,advanced:'{}',strict:false};
describe('Rust request boundary',()=>{
  it('preserves separate engines and IPv6 host placement',()=>{const req=requestFor('path',{...input,family:'IPv6',engine:'path_trace'});expect(req.capability).toBe('path_trace');expect(req.layers[0]).toMatchObject({hostsIPv4:[],hostsIPv6:['localhost'],types:['ICMP']});});
  it('keeps baseline composition in Rust and preserves explicit zero',()=>{const req=requestFor('baseline',input);expect(req.workflow).toBe('baseline');expect(req.layers[0]).toMatchObject({throughput:{maxTotalTests:0}});});
  it('passes override layers separately and leaves semantic validation to Rust',()=>{expect(requestFor('throughput',{...input,advanced:'{"tcp_streams":[2,2],"unknown":true}'}).layers[1]).toEqual({tcp_streams:[2,2],unknown:true});});
  it('rejects nonobject JSON layers',()=>{for(const advanced of ['null','[]','true','invalid'])expect(()=>requestFor('triage',{...input,advanced})).toThrow();});
  it('invalidates review for target and output changes',()=>{const before=requestFor('path',input);expect(fingerprint(before,'a')).not.toBe(fingerprint(before,'b'));expect(fingerprint(before,'a')).not.toBe(fingerprint(requestFor('path',{...input,host:'other'}),'a'));});
  it('retains category in errors and escapes external values',()=>{expect(errorMessage({category:'permission',message:'Denied'})).toBe('permission: Denied');expect(escape('<img onerror="x">')).toBe('&lt;img onerror=&quot;x&quot;&gt;');});
  it('distinguishes cancellation in progress from completed cancellation',()=>{expect(active(null)).toBe(false);expect(active({state:'cancelling'} as never)).toBe(true);expect(active({state:'cancelled'} as never)).toBe(false);});
});
