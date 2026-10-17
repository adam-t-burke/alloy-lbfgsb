"""Summarize raw driver experiments and independently verify percentage arithmetic."""
from decimal import Decimal
import hashlib
import json
import statistics
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
DIRECTORY=ROOT/'benchmarks/results/driver-profile'
REFERENCES={'timings':'original','candidates':'current','confirm':'current',
            'isolated-small':'current','final':'original','api-control':'equivalent-original',
            'direct':'equivalent-original','inline':'equivalent-original'}

def signature(row):
    return tuple(row[key] for key in ['iterations','evaluations','termination',
        'final_f','final_pg','x_bits_hash','g_bits_hash'])

def exact_median(values):
    ordered=sorted(Decimal(str(v)) for v in values)
    middle=len(ordered)//2
    return ordered[middle] if len(ordered)%2 else (ordered[middle-1]+ordered[middle])/2

def summarize():
    output={}
    all_signatures={}
    checked=0
    for label,reference in REFERENCES.items():
        data=json.loads((DIRECTORY/(label+'.json')).read_text())
        groups={}
        for row in data['records']:
            key=(row['objective'],row['n'],row['m'],row['backend'])
            groups.setdefault(key,[]).append(row)
            all_signatures.setdefault(key,set()).add(signature(row))
        summaries=[]
        for key,rows in groups.items():
            variants={}
            for variant in sorted({r['variant'] for r in rows}):
                values=[r['microseconds'] for r in rows if r['variant']==variant]
                variants[variant]={'median_us':statistics.median(values),'min_us':min(values),
                    'max_us':max(values),'samples':len(values)}
            for variant,values in variants.items():
                change=100*(values['median_us']/variants[reference]['median_us']-1)
                values['change_from_reference_percent']=change
                # Independent decimal median and difference / baseline expression.
                original=exact_median([r['microseconds'] for r in rows if r['variant']==reference])
                actual=exact_median([r['microseconds'] for r in rows if r['variant']==variant])
                recomputed=(actual-original)*Decimal(100)/original
                error=abs(Decimal(str(change))-recomputed)
                assert error <= Decimal('1e-10'),(key,variant,error)
                values['percentage_check_absolute_error']=float(error)
                checked+=1
            summaries.append({'objective':key[0],'n':key[1],'m':key[2],'backend':key[3],
                'reference':reference,'variants':variants})
        output[label]={'environment':data['environment'],'cases':summaries}
    assert all(len(values)==1 for values in all_signatures.values()),all_signatures
    output['verification']={'percentage_checks_passed':checked,'absolute_tolerance_percentage_points':1e-10,
        'fingerprint_case_backend_groups_passed':len(all_signatures),
        'meaning':'Final x/gradient bit fingerprints and reported values/counters match across all experiments; not a mathematical proof for untested inputs.'}
    output['build']={'cpu':'Intel Core Ultra 9 285K','rustc':'1.89.0','opt_level':3,'lto':True,'codegen_units':1,
        'baseline_commit':'9a200cb','resumable_commit':'ca6baa5','features':'faer-backend; benchmark-instrumentation only in instrumented.json',
        'benchmark_scope':'Whole warmed solve invocation, including input reset and common checksum/fingerprints; separate instrumentation is not used for speed claims.',
        'harness_canonical_sha256':hashlib.sha256((ROOT/'tools/profile_bench.rs').read_text().encode()).hexdigest(),
        'patches':{p.name:hashlib.sha256(p.read_text().encode()).hexdigest() for p in (ROOT/'tools/variants').glob('*.patch')}}
    (DIRECTORY/'summary.json').write_text(json.dumps(output,indent=2)+'\n')
    print('PASS:',output['verification'])
    for label in ['final','api-control']:
        print(label)
        for case in output[label]['cases']:
            print(case['n'],case['backend'],round(case['variants']['isolated']['change_from_reference_percent'],3))

if __name__=='__main__':
    summarize()
