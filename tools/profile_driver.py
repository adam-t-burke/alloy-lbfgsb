"""Controlled Windows/Linux driver comparison; run with uv, no Python dependencies.

Prepare isolated worktrees first (see ADR-002). Run --build, then --run; --profile
builds and measures separately instrumented binaries. No production source is edited.
"""
import argparse
import csv
import ctypes
import io
import json
import os
import platform
import statistics
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VARIANTS = ['original', 'current', 'no-fill']
CASES = [('rosenbrock',25,5), ('rosenbrock',1000,10), ('rosenbrock',8192,10),
         ('mixed-bounds',100000,10), ('active-corner',1000000,10)]
BACKENDS = ['deterministic','faer']

def pin_cpu(cpu):
    if os.name == 'nt':
        kernel = ctypes.WinDLL('kernel32',use_last_error=True)
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        kernel.SetProcessAffinityMask.argtypes = [ctypes.c_void_p,ctypes.c_size_t]
        if not kernel.SetProcessAffinityMask(kernel.GetCurrentProcess(),1 << cpu):
            raise ctypes.WinError(ctypes.get_last_error())
    else:
        os.sched_setaffinity(0,{cpu})

def build(variant,profile):
    root = ROOT/'target'/('profile-'+variant)
    command = ['cargo','bench','--locked','--manifest-path',str(root/'Cargo.toml'),
               '--bench','performance','--features','faer-backend'+(',benchmark-instrumentation' if profile else ''),
               '--no-run','--message-format=json']
    result = subprocess.run(command,cwd=ROOT,text=True,capture_output=True,check=True)
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get('reason')=='compiler-artifact' and item.get('target',{}).get('kind')==['bench']:
            if item.get('executable'):
                return item['executable']
    raise RuntimeError('benchmark executable not found: '+result.stderr)

def run(args):
    global VARIANTS
    VARIANTS = args.variants.split(',')
    directory = ROOT/'benchmarks/results/driver-profile'
    directory.mkdir(parents=True,exist_ok=True)
    stem = args.label or ('instrumented' if args.profile else 'timings')
    manifest=directory/(stem+'-binaries.json' if args.label or args.profile else 'binaries.json')
    if args.prepare:
        for variant in VARIANTS:
            checkout=ROOT/'target'/('profile-'+variant)
            if checkout.exists():
                raise RuntimeError(f'{checkout} already exists; preserve or remove the old experiment before preparing')
            revision='9a200cb' if variant in ['original','equivalent-original'] else 'ca6baa5'
            subprocess.run(['git','worktree','add','--detach',str(checkout),revision],cwd=ROOT,check=True)
            (checkout/'Cargo.lock').write_bytes((ROOT/'Cargo.lock').read_bytes())
            (checkout/'benches/performance.rs').write_bytes((ROOT/'tools/profile_bench.rs').read_bytes())
            patch=ROOT/'tools/variants'/(variant+'.patch')
            if patch.exists():
                subprocess.run(['git','apply',str(patch)],cwd=checkout,check=True)
        if not args.build and not args.run: return
    if args.build:
        binaries={variant:build(variant,args.profile) for variant in VARIANTS}
        manifest.write_text(json.dumps(binaries,indent=2)+'\n')
        print('Built '+', '.join(binaries),flush=True)
        if not args.run: return
    binaries=json.loads(manifest.read_text())
    pin_cpu(args.cpu)
    cases=CASES if not args.profile else [CASES[1],CASES[3]]
    if args.cases:
        selected=set(args.cases.split(','))
        cases=[case for case in cases if str(case[1]) in selected]
    backends=args.backends.split(',')
    repeats=args.repeats
    records=[]
    environment=dict(os.environ,LBFGSB_BENCH_MILLIS=str(args.millis))
    output=directory/(stem+'.json')
    for case_index,(objective,n,m) in enumerate(cases):
        for backend in backends:
            for repeat in range(repeats):
                offset=(repeat+case_index)%len(VARIANTS)
                order=VARIANTS[offset:]+VARIANTS[:offset]
                for variant in order:
                    command=[binaries[variant],f'--objective={objective}',f'--n={n}',f'--m={m}',f'--backend={backend}']
                    result=subprocess.run(command,cwd=ROOT,env=environment,text=True,capture_output=True,check=True)
                    row=list(csv.DictReader(io.StringIO(result.stdout)))[-1]
                    record=dict(row,variant=variant,repeat=repeat+1,objective=objective,n=n,m=m,backend=backend)
                    record['microseconds']=float(record['microseconds'])
                    record['runs']=int(record['runs'])
                    record['iterations']=int(record['iterations'])
                    record['evaluations']=int(record['evaluations'])
                    for line in result.stderr.splitlines():
                        if line.startswith('profile_ns,'):
                            keys=['total','evaluation','direction','history','observer','residual','cauchy','freev','formk','cmprlb_subsm']
                            record['phase_nanoseconds']=dict(zip(keys,map(float,line.split(',')[1:])))
                    records.append(record)
                    output.write_text(json.dumps(dict(environment={'cpu_affinity':args.cpu,'platform':platform.platform(),
                        'minimum_sample_ms':args.millis,'repeats':repeats,'profile':args.profile},records=records),indent=2)+'\n')
                print(f'{objective} n={n} m={m} {backend} repeat={repeat+1}/{repeats}',flush=True)
    # All coordinates and gradients must match bitwise within the same backend/case.
    for objective,n,m in cases:
        for backend in backends:
            rows=[r for r in records if (r['objective'],r['n'],r['m'],r['backend'])==(objective,n,m,backend)]
            signatures={(r['iterations'],r['evaluations'],r['termination'],r['final_f'],r['final_pg'],r['x_bits_hash'],r['g_bits_hash']) for r in rows}
            assert len(signatures)==1,(objective,n,backend,signatures)
            medians={v:statistics.median(r['microseconds'] for r in rows if r['variant']==v) for v in VARIANTS}
            print(objective,n,backend,medians,flush=True)
    print('PASS: all variant fingerprints and counters match.',flush=True)

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--prepare',action='store_true')
    parser.add_argument('--build',action='store_true')
    parser.add_argument('--run',action='store_true')
    parser.add_argument('--profile',action='store_true')
    parser.add_argument('--cases',help='comma-separated dimensions from the fixed case matrix')
    parser.add_argument('--backends',default='deterministic,faer')
    parser.add_argument('--variants',default='original,current,no-fill')
    parser.add_argument('--label')
    parser.add_argument('--cpu',type=int,default=2)
    parser.add_argument('--repeats',type=int,default=5)
    parser.add_argument('--millis',type=int,default=750)
    run(parser.parse_args())
