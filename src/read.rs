use super::{Kmer, Repr, CorrMap, Params, minimizers, RacyBloom, REVCOMP_AWARE, utils, poa};
use nthash::{NtHashIterator};
use std::collections::{HashMap,HashSet};
//use std::collections::hash_map::Entry::{Occupied, Vacant};
//use std::collections::VecDeque;
use super::utils::pretty_minvec;
type Buckets<'a> = HashMap<Vec<u64>, Vec<String>>;
#[derive(Clone, Default)]
pub struct Read {
    pub id: String,
    pub minimizers : Vec<String>,
    pub minimizers_pos: Vec<usize>,
    pub transformed: Vec<u64>,
    pub seq: String, 
    //pub seq_str: &'a str, // an attempt to avoid copying the string sequence returned by the fasta parser (seems too much effort to implement for now)
    pub corrected: bool
}
#[derive(Clone)]
pub struct Lmer {
    pub pos: usize,
    pub hash: u64
}

impl Read {
    pub fn extract(inp_id: &str, inp_seq: String, params: &Params, minimizer_to_int: &HashMap<String, u64>, uhs_bloom: &RacyBloom, lcp_bloom: &RacyBloom) -> Self {
        if params.uhs {Read::extract_uhs(inp_id, inp_seq, params, minimizer_to_int, uhs_bloom)}
        else if params.lcp {Read::extract_lcp(inp_id, inp_seq, params, minimizer_to_int, lcp_bloom)}
        else {Read::extract_density(inp_id, inp_seq, params, minimizer_to_int)}
    }

    //delete
    pub fn extract_lcp(inp_id: &str, inp_seq_raw: String, params: &Params, minimizer_to_int: &HashMap<String, u64>, lcp_bloom: &RacyBloom) -> Self {
        let density = params.density;
        let l = params.l;
        let read_minimizers = Vec::<String>::new();
        let mut read_minimizers_pos = Vec::<usize>::new();
        let mut read_transformed = Vec::<u64>::new();
        let hash_bound = ((density as f64) * (u64::max_value() as f64)) as u64;
        let tup;
        let inp_seq;
        if !params.use_hpc {
            tup = Read::encode_rle(&inp_seq_raw); //get HPC sequence and positions in the raw nonHPCd sequence
            inp_seq = tup.0; //assign new HPCd sequence as input
        }
        else {
            inp_seq = inp_seq_raw.clone(); //already HPCd before so get the raw sequence
        }
        let iter = NtHashIterator::new(inp_seq.as_bytes(), l).unwrap().enumerate().filter(|(_i, x)| *x <= hash_bound);
        for (i, mut hash) in iter {
            let lmer = &inp_seq[i..i+l];
            if lmer.contains('N') {continue;}
            if params.error_correct || params.has_lmer_counts {
                let res = minimizer_to_int.get(lmer); // allows to take the 'skip' array into account
                if res.is_none() {continue;} // possible discrepancy between what's calculated in minimizers_preparation() and here
                hash = *res.unwrap();
            }
            if lcp_bloom.get().check_and_add(&hash) {
                read_minimizers_pos.push(i);
                read_transformed.push(hash);
            }
        }
        Read {id: inp_id.to_string(), minimizers: read_minimizers, minimizers_pos: read_minimizers_pos, transformed: read_transformed, seq: inp_seq_raw, corrected: false}
    }
    pub fn extract_uhs(inp_id: &str, inp_seq_raw: String, params: &Params, minimizer_to_int: &HashMap<String, u64>, uhs_bloom: &RacyBloom) -> Self {
        let density = params.density;
        let l = params.l;
        let read_minimizers = Vec::<String>::new();
        let mut read_minimizers_pos = Vec::<usize>::new();
        let mut read_transformed = Vec::<u64>::new();
        let hash_bound = ((density as f64) * (u64::max_value() as f64)) as u64;
        let tup;
        let inp_seq;
        if !params.use_hpc {
            tup = Read::encode_rle(&inp_seq_raw); //get HPC sequence and positions in the raw nonHPCd sequence
            inp_seq = tup.0; //assign new HPCd sequence as input
        }
        else {
            inp_seq = inp_seq_raw.clone(); //already HPCd before so get the raw sequence
        }
        let iter = NtHashIterator::new(inp_seq.as_bytes(), l).unwrap().enumerate().filter(|(_i, x)| *x <= hash_bound);
        for (i, hash) in iter {
            let lmer = &inp_seq[i..i+l];
            let mut hash : u64 = hash;
            if params.error_correct || params.has_lmer_counts {
                let res = minimizer_to_int.get(lmer); // allows to take the 'skip' array into account
                if res.is_none() {continue;} // possible discrepancy between what's calculated in minimizers_preparation() and here
                hash = *res.unwrap();
            }
            if uhs_bloom.get().check_and_add(&hash) {
                read_minimizers_pos.push(i);
                read_transformed.push(hash);
            }
        }
        Read {id: inp_id.to_string(), minimizers: read_minimizers, minimizers_pos: read_minimizers_pos, transformed: read_transformed, seq: inp_seq_raw, corrected: false}
    }
    pub fn encode_rle(inp_seq: &str) -> (String, Vec<usize>) {
        let mut prev_char = '#';
        let mut hpc_seq = String::new();
        let mut pos_vec = Vec::<usize>::new();
        let mut prev_i = 0;
        for (i, c) in inp_seq.chars().enumerate() {
            if c == prev_char && "ACTGactgNn".contains(c) {continue;}
            if prev_char != '#' {
                hpc_seq.push(prev_char);
                pos_vec.push(prev_i);
                prev_i = i;
            }
            prev_char = c;
        }
        hpc_seq.push(prev_char);
        pos_vec.push(prev_i);
        (hpc_seq, pos_vec)
    }
    
    pub fn extract_density(inp_id: &str, inp_seq_raw: String, params: &Params, minimizer_to_int: &HashMap<String, u64>) -> Self {
        let density = params.density;
        let l = params.l;
        let read_minimizers = Vec::<String>::new();
        let mut read_minimizers_pos = Vec::<usize>::new();
        let mut read_transformed = Vec::<u64>::new();
        //println!("parsing new read: {}\n",inp_seq);
        let hash_bound = ((density as f64) * (u64::max_value() as f64)) as u64;
        let mut tup = (String::new(), Vec::<usize>::new());
        let inp_seq;
        if !params.use_hpc {
            tup = Read::encode_rle(&inp_seq_raw); //get HPC sequence and positions in the raw nonHPCd sequence
            inp_seq = tup.0; //assign new HPCd sequence as input
        }
        else {
            inp_seq = inp_seq_raw.clone(); //already HPCd before so get the raw sequence
        }
        if inp_seq.len() < l {
            return Read {id: inp_id.to_string(), minimizers: read_minimizers, minimizers_pos: read_minimizers_pos, transformed: read_transformed, seq: inp_seq_raw, corrected: false};
        }
        let iter = NtHashIterator::new(inp_seq.as_bytes(), l).unwrap().enumerate().filter(|(_i, x)| *x <= hash_bound);
        for (i,hash) in iter {
            let lmer = &inp_seq[i..i+l];
            let mut hash :u64 = hash;
            if params.error_correct || params.has_lmer_counts {
                let res = minimizer_to_int.get(lmer); // allows to take the 'skip' array into account
                if res.is_none() {continue;} // possible discrepancy between what's calculated in minimizers_preparation() and here
                hash = *res.unwrap();
            }
            if !params.use_hpc {read_minimizers_pos.push(tup.1[i]);} //if not HPCd need raw sequence positions
            else {read_minimizers_pos.push(i);} //already HPCd so positions are the same
            read_transformed.push(hash);
        }
        Read {id: inp_id.to_string(), minimizers: read_minimizers, minimizers_pos: read_minimizers_pos, transformed: read_transformed, seq: inp_seq_raw, corrected: false}
    }

    pub fn label(&self, read_seq: String, read_minimizers: Vec<String>, read_minimizers_pos: Vec<usize>, read_transformed: Vec<u64>, corrected_map: &mut CorrMap) {
        corrected_map.insert(self.id.to_string(), (read_seq, read_minimizers, read_minimizers_pos, read_transformed));
    }

    pub fn read_to_kmers(&mut self, params: &Params) -> Repr {
        let k = params.k;
        let l = params.l;
        let mut output : Repr = Vec::new();
        for i in 0..(self.transformed.len()- k + 1) {
            let mut node : Kmer = Kmer::make_from(&self.transformed[i..i+k]);
            let mut seq_reversed = false;
            if REVCOMP_AWARE { 
                let (node_norm, reversed) = node.normalize(); 
                node = node_norm;
                seq_reversed = reversed;
            } 
            let seq = self.seq[self.minimizers_pos[i] as usize..(self.minimizers_pos[i+k-1] as usize + l)].to_string();
            //if seq_reversed {
            //    seq = utils::revcomp(&seq);
            //}

            /* // TODO incorporate that code somehow into writing an adequate sequence 
              // not just the first sequence that appears for a kmer (at abundance 2)
              // but rather the seq that has closest length to median length for that kmer
               let mut inserted = false;
               if kmer_seqs.contains_key(&node) { 
               let median_seq_len : usize = median(kmer_seqs_lens.get(&node).unwrap()) as usize;
            //println!("node: {} seqlen: {}",node.print_as_string(),seq.len());
            // insert that sequence if it's closer to the median length than the current
            // inserted string
            if ((seq.len() - median_seq_len) as f64).abs() < ((kmer_seqs.get(&node).unwrap().len() - median_seq_len) as f64).abs()
            { 
            kmer_seqs.insert(node.clone(), seq.clone());
            inserted = true;
            }
            }
            else
            {
            kmer_seqs.insert(node.clone(), seq.clone());
            inserted = true;
            }
            kmer_seqs_lens.entry(node.clone()).or_insert(Vec::new()).push(seq.len() as u32);
            */

            let origin = "*".to_string(); // uncomment the line below to track where the kmer is coming from (but not needed in production)
            //let origin = format!("{}_{}_{}", self.id, self.minimizers_pos[i].to_string(), self.minimizers_pos[i+k-1].to_string()); 

            let position_of_second_minimizer = match seq_reversed {
                true => self.minimizers_pos[i+k-1] - self.minimizers_pos[i+k-2],
                false => self.minimizers_pos[i+1] - self.minimizers_pos[i]
            };
            let position_of_second_to_last_minimizer = match seq_reversed {
                true => self.minimizers_pos[i+1] - self.minimizers_pos[i],
                false => self.minimizers_pos[i+k-1] - self.minimizers_pos[i+k-2]
            };
            let shift = (position_of_second_minimizer, position_of_second_to_last_minimizer);
            output.push((node, seq, seq_reversed, origin, shift));
        }
        output
    }
    pub fn poa_correct(&mut self, int_to_minimizer: &HashMap<u64, String>, poa_map: &mut HashMap<String, Vec<String>>, buckets: &Buckets, params : &Params, mut corrected_map: &mut CorrMap, reads_by_id: &HashMap<String, Read>) {

	// poa alignment scoring parameters
	let score = |a: u64, b: u64| if a == b {1i32} else {-1i32};
        let scoring = poa::Scoring::new(-1, -1, score);
        //let scoring = poa::Scoring::new(-1, 0, score); // the hope is that a 0 gap extend penalty somehow approximates semiglobal, but that's not quite true
        // other alignment parameters
        let dist_threshold = 0.15; // mash distance cut-off for read recruitment
        //let top_x_aligned_reads = 0; // get the 10 best read alignments per template
        let poa_global_min_score = std::i32::MIN; // discard all alignments below that score (discarded when top_X_aligned_read > 0)
        //let mut poa_global_min_score = -10; 
        let debug = params.debug;
        let n = params.n;
        let l = params.l;
        let read_minimizers_pos = &self.minimizers_pos;
        let read_transformed = &self.transformed;
        let seq_id = &self.id;
        let seq_str = &self.seq;
        let mut added_reads : HashSet<String> = HashSet::new();
        let mut bucket_reads = Vec::<&Read>::new();
        let mut poa_ids = Vec::<String>::new();
        let mut aligner = poa::Aligner::new(scoring, &read_transformed, Some(seq_str), Some(read_minimizers_pos));
        // populate bucket_reads with reads that share n consecutive minimizers with template
        added_reads.insert(self.id.clone());  
        for i in 0..read_transformed.len()-n+1 {
            let bucket_idx = &utils::normalize_vec(&read_transformed[i..i+n].to_vec());
            let entry = &buckets[bucket_idx];
            for id in entry.iter() {
                let query = &reads_by_id[id];
                if !added_reads.contains(&query.id) {
                    bucket_reads.push(query);
                    added_reads.insert(query.id.clone());  
                }
            }   
        }
        // filter bucket_reads so that it only contains reads below a mash distance of template
        let mut bucket_reads : Vec<(&Read, f64)> = bucket_reads.iter().map(|seq| (*seq, minimizers::dist(self, seq, &params))).filter(|(_seq, dist)| *dist < dist_threshold).collect();
        // sort reads by their distance
        bucket_reads.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
        let max_poa_reads = 80;
        if bucket_reads.len() > max_poa_reads {
            bucket_reads = bucket_reads[..max_poa_reads].to_vec(); // limit to 50 lowest-distance reads per POA correction
        }
        //if bucket_reads.len() == 0 {println!("Read has no neighbors");}
        // do a first pass aligning reads to the template to get only the top X best scoring reads
        // TODO this code isn't up to date as it only aligns in forward direction
        /*
        let mut alignment_scores = Vec::new();
        if top_X_aligned_reads > 0 
        {
            for i in 0..bucket_reads.len() {
                aligner.global(&bucket_reads[i].0.transformed);
                let score = aligner.alignment().score;
                alignment_scores.push((score,i));
            }
            // sort alignment scores decreasing
            alignment_scores.sort_by(|a, b| b.cmp(a));
            //if debug { println!("read alignment scores: {:?}",alignment_scores); }

            // threshold becomes the X-th highest alignment
            if alignment_scores.len() > top_X_aligned_reads
            {
                poa_global_min_score = alignment_scores[top_X_aligned_reads].0;
            }
            else{
                poa_global_min_score = std::i32::MIN;
            }
        }
        */
        let mut nb_aln_forward = 0;
        let mut nb_aln_backward = 0;
        for bucket_read in &bucket_reads {
            poa_ids.push(bucket_read.0.id.to_string());
            // don't know how to save alignments so i'll just waste time and recompute the best
            // scoring alignment.
            // it's a TODO optimization
            let read = &bucket_read.0;
            let seq = &read.seq;
            let pos = &read.minimizers_pos;
            aligner.semiglobal(&read.transformed, Some(seq), Some(pos));
            let fwd_score = aligner.alignment().score;
            if debug {println!("--- Forward alignment score: {} (ID: {})\nMinimizer-space: {}\n{}\n---", aligner.alignment().score, read.id.to_string(), pretty_minvec(&read.transformed), aligner.print_aln());}
            let mut rev_read = read.transformed.clone();
            rev_read.reverse();
            let rev_seq = utils::revcomp(&seq);
            let mut rev_minim_pos = pos.clone();
            rev_minim_pos.reverse();
            for pos in &mut rev_minim_pos {
                *pos = seq.len() - l - *pos;
            }
            aligner.semiglobal(&rev_read, Some(&rev_seq), Some(&rev_minim_pos));
            if debug { println!("--- Backward alignment score: {} (ID: {})\nMinimizer-space: {}\n{}\n---", aligner.alignment().score, read.id.to_string(), pretty_minvec(&rev_read), aligner.print_aln());}
            let bwd_score = aligner.alignment().score;
            //let mut aln_ori = "";
            if std::cmp::max(fwd_score, bwd_score) >= poa_global_min_score { 
               if fwd_score > bwd_score { 
                    aligner.semiglobal(&read.transformed, Some(seq), Some(pos));
                    //aln_ori = "fwd";
                    nb_aln_forward += 1;
                } else { 
                    aligner.semiglobal(&rev_read, Some(&rev_seq), Some(&rev_minim_pos));
                    //aln_ori = "bwd";
                    nb_aln_backward += 1;
                }
                aligner.add_to_graph(); 
            }
            //if debug { aligner.traceback.print(aligner.graph(), bucket_reads[i].0.transformed.clone()); } // prints a confusing traceback (and crashes)
        }
        let (consensus, consensus_edge_seqs) = aligner.poa.consensus(&params);
        //println!("consensus()/consensus_edge_seqs() lens: {} / {}", consensus.len(), consensus_edge_seqs.len());
        let (consensus, consensus_edge_seqs) = aligner.consensus_boundary(&consensus, &consensus_edge_seqs, &read_transformed, debug);
        let consensus_read = consensus.iter().map(|minim| int_to_minimizer[minim].to_string()).collect::<Vec<String>>();
        let len_before_poa  = self.transformed.len();
        let len_after_poa  = consensus_read.len();
        if debug { println!("Length of template before/after POA: {} / {} (ID: {})", len_before_poa, len_after_poa, seq_id);}
        if debug { println!("Number of bucketed reads aligned forwards/backwards: {} / {} ", nb_aln_forward, nb_aln_backward);}
        let mut consensus_str = String::new();
        let mut pos_idx = 0;
        let mut consensus_pos = Vec::<usize>::new();
        if consensus.is_empty() {return}
        for insert in consensus_edge_seqs.iter() {
            consensus_pos.push(pos_idx);
            consensus_str.push_str(insert);
            pos_idx += insert.len();
        }
        consensus_pos.push(pos_idx);
        consensus_str.push_str(&int_to_minimizer[&consensus[consensus.len()-1]]);
        let mut corrected_count = 0;
        let mut threshold = params.correction_threshold;
        if params.correction_threshold == 0 {threshold = 0 as i32;}
        for (read, _dist) in bucket_reads.iter_mut() {
            if corrected_count >= threshold {break;}
            if !read.corrected {
                read.label(consensus_str.to_string(), consensus_read.to_vec(), consensus_pos.to_vec(), consensus.to_vec(), &mut corrected_map);
                corrected_count += 1;
            }
        }
        poa_map.insert(seq_id.to_string(), poa_ids.to_vec());
        self.seq = consensus_str;
        self.minimizers = consensus_read;
        self.minimizers_pos = consensus_pos;
        self.transformed = consensus;
        self.corrected = true;
    }
}
