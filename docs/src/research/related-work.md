# Related Work Report: Intent-Predictive Operating Systems (Early)

> **Project context**: DiPECS — a cloud-LLM-driven distributed intent operating system prototype targeting Android API 33, with privacy air-gap enforcement, deterministic state machine transitions, and strict mechanism-policy separation.
>
> **Date**: 2026-03-19

---

## Table of Contents

1. [LLM-Driven / AI Operating Systems](#1-llm-driven--ai-operating-systems)
2. [Mobile App & Intent Prediction Systems](#2-mobile-app--intent-prediction-systems)
3. [Privacy-Preserving AI for User Behavior](#3-privacy-preserving-ai-for-user-behavior)
4. [Cloud-Edge Hybrid OS & Proactive Systems](#4-cloud-edge-hybrid-os--proactive-systems)
5. [Comparative Analysis](#5-comparative-analysis)
6. [Benchmark Landscape Summary](#6-benchmark-landscape-summary)
7. [Differentiation of DiPECS](#7-differentiation-of-dipecs)
8. [References](#8-references)

---

## 1. LLM-Driven / AI Operating Systems

### 1.1 AIOS: LLM Agent Operating System

- **Authors**: Kai Mei, Xi Zhu, Wujiang Xu, Wenyue Hua, Mingyu Jin, Zelong Li, Shuyuan Xu, Ruosong Ye, Yingqiang Ge, Yongfeng Zhang
- **Year**: 2024 (accepted COLM 2025)
- **Paper**: arXiv:2403.16971

**Core approach**: OS kernel architecture that isolates LLM-specific services (scheduling, context management, memory management, storage, access control) from agent application logic. Provides an SDK so agent frameworks (ReAct, Reflexion, Autogen, MetaGPT) can run concurrently atop a shared LLM resource managed by the kernel.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Agent success rate (HumanEval, ReAct) | 48.8% → 50.6% with AIOS |
| Agent success rate (GAIA, Autogen) | 7.3% → 9.7% |
| Agent success rate (MINT-Code, Reflexion) | 32.4% → 33.8% |
| Throughput improvement | Up to 2.1× (Reflexion on Llama-3.1-8b) |
| Scalability | Linear scaling from 250 to 2,000 concurrent agents |
| Hardware | Single NVIDIA RTX A5000 (24 GB) |
| LLMs tested | GPT-4o-mini, Llama-3.1-8b, Mistral-7b |

**Limitations**: Modest accuracy improvements (1–2 pp); no cross-machine distributed scheduling; no privacy layer.

---

### 1.2 OS-Copilot (FRIDAY): Towards Generalist Computer Agents with Self-Improvement

- **Authors**: Zhiyong Wu, Chengcheng Han, Zichen Ding, Zhenmin Weng, Zhoumianze Liu, Shunyu Yao, Tao Yu, Lingpeng Kong
- **Year**: 2024
- **Paper**: arXiv:2402.07456

**Core approach**: Framework for building generalist agents that interact across all OS elements — web, terminal, filesystem, multimedia, third-party apps. Accumulates reusable skills and self-improves.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| GAIA Level 1 success rate | 40.86% (35% relative improvement over prior SOTA 30.3%) |
| GAIA Level 3 success rate | 6.12% (prior systems scored 0%) |
| SheetCopilot-20 | 60% (vs. GPT-4 baseline 55%) |
| Self-improvement pass rate | 64.6% → 83.3% after self-directed learning |

**Limitations**: Relies on GPT-4 as backbone (cost/latency). No mobile or embedded OS evaluation.

---

### 1.3 UFO / UFO2: UI-Focused Agent for Windows OS Interaction

- **Authors**: Chaoyun Zhang et al. (Microsoft Research)
- **Year**: 2024 (UFO), 2025 (UFO2)
- **Papers**: arXiv:2402.07939, arXiv:2504.14603

**Core approach**: Dual-agent architecture (HostAgent + AppAgent) where GPT-Vision observes Windows GUI via UI Automation APIs. UFO2 extends to a full "Desktop AgentOS" with hybrid GUI-API control and speculative multi-action planning.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Overall success rate | 86% (vs. GPT-4 baseline 42%, GPT-3.5 baseline 24%) |
| Completion rate | 89.6% (vs. GPT-4's 47.8%) |
| Average steps per task | 5.48 |
| Cross-application tasks | 80% success, 9.8 avg steps |
| Per-app: Outlook, Word, Explorer, WeChat | 100% each |
| Per-app: Photos, Edge | 80% each |
| Per-app: Adobe Acrobat | 60% |
| Safeguard activation rate | 85.7% |

**Limitations**: Windows-only; tightly coupled to UI Automation backend; struggles with apps lacking UIA support.

---

### 1.4 SWE-agent: Agent-Computer Interfaces for Automated Software Engineering

- **Authors**: John Yang et al. (Princeton University)
- **Year**: 2024 (NeurIPS 2024)
- **Paper**: arXiv:2405.15793

**Core approach**: Introduces Agent-Computer Interfaces (ACI) — curated commands with guardrails for LM agents to explore repositories and fix bugs autonomously.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| SWE-bench (2,294 real GitHub issues) | 12.47% pass@1 (prior SOTA: 3.8%) |
| HumanEvalFix | 87.7% pass@1 |
| mini-swe-agent (100-line variant) | >74% on SWE-bench Verified |

**Limitations**: 87.5% of issues still unsolved; high token cost; evaluated primarily on Python repos.

---

### 1.5 MemGPT: Towards LLMs as Operating Systems

- **Authors**: Charles Packer et al. (UC Berkeley)
- **Year**: 2023 (revised February 2024)
- **Paper**: arXiv:2310.08560

**Core approach**: OS virtual memory analogy for LLM context management — hierarchical memory (main context = RAM, archival = disk) with self-directed paging and interrupt-driven control flow.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Deep Memory Retrieval accuracy | 93.4% (vs. recursive summarization baseline 35.3%) |
| Document QA | Scales to arbitrary document sizes via archival paging |
| Multi-session chat | Significantly outperforms fixed-context baselines |

**Limitations**: Every memory operation adds LLM inference cost; no formal memory consistency guarantees.

---

### 1.6 LLM as OS, Agents as Apps (Vision Paper)

- **Authors**: Yingqiang Ge et al. (Rutgers University)
- **Year**: 2023
- **Paper**: arXiv:2312.03815

**Core approach**: Conceptual taxonomy framing the LLM as an OS kernel providing system calls for agent applications. Distinguishes System Agents (OS services) from User Agents (application tasks).

**Benchmarks**: None — purely a position/vision paper. Lays the conceptual foundation for the AIOS implementation.

---

## 2. Mobile App & Intent Prediction Systems

### 2.1 FALCON: Fast App Launching via Predictive User Context

- **Authors**: Tingxin Yan, David Chu, Deepak Ganesan, Aman Kansal, Jie Liu
- **Year**: 2012
- **Venue**: ACM MobiSys

**Core approach**: Predicts next app launch using contextual signals (location, time-of-day, sensors). Cost-benefit learning algorithm weighs preloading benefit against energy cost.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Latency savings | ~6 seconds per app startup |
| Energy overhead | ≤2% daily battery |
| Content staleness at launch | ~3 minutes |
| Participants | 16 users (Windows Phone) |

**Limitations**: Windows Phone only; 16 users; no inter-app sequential modeling.

---

### 2.2 AppUsage2Vec: Modeling Smartphone App Usage for Prediction

- **Authors**: Sha Zhao et al.
- **Year**: 2019
- **Venue**: IEEE ICDE

**Core approach**: Doc2Vec-inspired model with app-attention mechanism encoding user personalization, temporal context, and app-transition patterns into a unified embedding space.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Dataset scale | 10,360 users, 46.4M records, 3 months |
| Evaluation metrics | HR@K, MRR@K (K=1,3,5) |
| Status | Widely used as a baseline by subsequent works |

**Limitations**: Non-public carrier-side dataset; no rich multi-modal context.

---

### 2.3 DeepAPP: Deep Reinforcement Learning for App Usage Prediction

- **Authors**: Zhihao Shen et al.
- **Year**: 2019
- **Venue**: ACM SenSys

**Core approach**: Model-free deep RL agent predicting next app from complex environmental context, with online adaptation and lightweight personalized agents.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Precision | 70.6% |
| Recall | 62.4% |
| Inference speed | 6.58× faster than prior SOTA |
| Field study satisfaction | 87.51% (29 participants) |
| Time savings agreement | 71.88% of participants |

**Limitations**: Only 29-user field study; cold-start problem unaddressed.

---

### 2.4 Atten-Transformer: Progressive Temporal Attention for App Prediction

- **Authors**: Longlong Li, Cunquan Qu, Guanghui Wang (Shandong University)
- **Year**: 2025
- **Paper**: arXiv:2502.16957

**Core approach**: Integrates sinusoidal temporal positional encoding with time-aware attention inside a Transformer network, dynamically weighting critical usage moments.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| HR@1 improvement over Appformer | +49.95% (Tsinghua dataset) |
| HR@1 improvement over MAPLE | +18.25% (LSapp, cold-start) |
| Tsinghua dataset | 1,000 users, 2,000 apps, 4.17M records, 7 days |
| LSapp dataset | 292 users, 87 apps, 599K records |
| Baselines compared | 14 methods (MFU, MRU, BPRMF, GRU4Rec, NeuSA, AppUsage2Vec, SR-GNN, DUGN, Appformer, MAPLE, CoSEM, TimesNet, FEDformer) |

**Limitations**: Performance plateaus after 6 days of history; overfitting risk; no multi-modal context.

---

### 2.5 GNN + Self-Attention Enhancement for Next App Prediction

- **Authors**: Junxin Chen et al.
- **Year**: 2025
- **Venue**: Scientific Reports (Nature)

**Core approach**: Combines gated graph neural networks (GGNN) for session dynamics with Transformer self-attention for long-term patterns. Dual graph structures for sequential and personalized interactions.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| HR@1 | 31.39% |
| HR@5 | 66.50% |
| MRR@5 | 46.58% |
| NDCG@5 | 52.08% |
| Dataset | 1,000 users, 2,000 apps, 4.17M records, 7 days |
| Baselines | MRU, MFU, Markov, FPMC, STAMP, GRU4Rec, SASRec, BERT4Rec, SRGNN, GCSAN |

**Limitations**: Only 7 days of data; no seasonal modeling; cold-start unaddressed.

---

### 2.6 Appformer: Progressive Multi-Modal Data Fusion

- **Authors**: Chuike Sun et al.
- **Year**: 2024
- **Paper**: arXiv:2407.19414

**Core approach**: Two-module Transformer architecture with cross-modal attention fusing app sequences, POI vectors, user IDs, and temporal features.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| HR@1 | 31.92% / 42.68% (two partitioning strategies) |
| Improvement over baselines | +4.75% / +7.89% |
| Additional metrics | MRR@K, NDCG@K, F1 (macro) |
| Dataset | Shanghai, 1 week (April 2016) |
| Baselines | RF, LR, XGBoost, MLP, LSTM, GRU, PCA+LSTM, NAP, AppUsage2Vec, DeepApp, PAULCI, DUGN |

**Limitations**: Single-city, single-week dataset; POI data requires base station association.

---

### 2.7 Anticipatory Mobile Computing (Survey)

- **Authors**: Veljko Pejovic, Mirco Musolesi
- **Year**: 2015
- **Venue**: ACM Computing Surveys, Vol. 47, No. 3

**Core approach**: Taxonomizes predictable phenomena (mobility, activity, health, social dynamics) and ML techniques (HMMs, Bayesian Networks, Markov processes). Defines the anticipatory pipeline: sense → infer context → predict → act proactively.

**Key challenges identified**: Energy efficiency, privacy/anonymity, non-deterministic behavior modeling, notification timing, distributed coordination, context drift adaptation.

---

## 3. Privacy-Preserving AI for User Behavior

### 3.1 SeqMF: Federated Privacy-Preserving Collaborative Filtering for Next App Prediction

- **Authors**: Albert Sayapin et al.
- **Year**: 2023 (published in *User Modeling and User-Adapted Interaction*, 2024)
- **Paper**: arXiv:2303.04744

**Core approach**: Sequential matrix factorization adapted for federated learning — raw usage data never leaves the device. Custom privacy mechanism protects gradient transmissions.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Static evaluation | Comparable to centralized baselines |
| Dynamic evaluation | Superior privacy-utility trade-off vs. frequency/sequential baselines |
| Privacy mechanism | Custom (non-standard DP; no epsilon reported) |

**Limitations**: Not superior in static settings; non-standard privacy mechanism complicates formal comparison.

---

### 3.2 Federated Learning for Mobile App Privacy Preferences

- **Authors**: Andre Brandao, Ricardo Mendes, Joao P. Vilela
- **Year**: 2022
- **Venue**: ACM CODASPY

**Core approach**: Privacy profiles via privacy-preserving clustering, then federated neural networks predict permission decisions (allow/deny).

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| F1-score | 0.9 (comparable to centralized baseline) |
| Accuracy loss from obfuscation | 0.76% |

**Limitations**: Narrow scope (permission preferences only); communication overhead not quantified.

---

### 3.3 Apple On-Device Intelligence (Siri Suggestions / Apple Intelligence)

- **Year**: 2016–present (Private Cloud Compute announced 2024)
- **Type**: Industry system (proprietary)

**Core approach**: On-device neural networks learn from local signals (Safari, emails, messages, contacts) for app prediction and contextual shortcuts. Neural Engine inference at <1W. When cloud LLM inference is needed, Apple routes through Private Cloud Compute with cryptographic guarantees — data is not stored or accessible to Apple. Federated learning for speaker recognition; differential privacy noise injected into local training.

**Benchmarks**: Internal A/B testing across hundreds of millions of devices. No public precision/recall numbers or epsilon values.

**Limitations**: Entirely proprietary; no reproducibility; tightly coupled to Apple hardware (Neural Engine, Secure Enclave).

---

## 4. Cloud-Edge Hybrid OS & Proactive Systems

### 4.1 Android App Standby Buckets & Adaptive Battery

- **Year**: 2018–present (Android 9+; Restricted bucket in Android 12)
- **Type**: Industry system (Google / DeepMind)

**Core approach**: ML-based classification of apps into five priority buckets (Active, Working Set, Frequent, Rare, Restricted) using an on-device TensorFlow Lite model predicting future usage probability. Based on bucket assignment, the OS throttles jobs, alarms, network access, and FCM message priority.

**Benchmarks & Evaluation**:

| Metric | Value |
| :--- | :--- |
| Restricted bucket limits | 1 job/day (10-min session), 1 alarm/day |
| Core constraint | ML model must consume less power than it saves |
| Inspection | `adb shell am get-standby-bucket` |

**Limitations**: OEM fragmentation (Samsung, Xiaomi customize behavior); no formal DP; aggressive bucketing can break background-dependent apps; no public benchmark.

---

### 4.2 RAMOS: Reference Architecture for Cloud-Edge Meta-Operating Systems

- **Authors**: Panagiotis Trakadas et al.
- **Year**: 2022
- **Venue**: *Sensors* (PMC 9692311)

**Core approach**: Peer-to-peer meta-OS transforming hierarchical cloud-edge-IoT into a dynamic distributed continuum. Data-centric orchestration with federated and swarm learning across heterogeneous devices.

**Benchmarks**: No empirical evaluation. Validated through five theoretical use-case scenarios only.

**Limitations**: No prototype, no implementation, no measured performance data.

---

### 4.3 Edge-Cloud Collaborative Computing Survey

- **Authors**: Jing Liu et al.
- **Year**: 2025
- **Paper**: arXiv:2505.01821

**Core approach**: Comprehensive survey across five layers: architectures, model optimization (compression, pruning, distillation), resource management, privacy/security, and deployments.

**Evaluation dimensions catalogued**: Latency, throughput, bandwidth efficiency, inference speed, model size reduction, accuracy preservation, power consumption, data protection effectiveness.

**Open challenges**: Accuracy-compression trade-offs; heterogeneous device management; federated learning with non-IID data; privacy mechanism overhead.

---

## 5. Comparative Analysis

### 5.1 System Feature Comparison

| System | Year | Platform | LLM-Driven | Privacy Layer | Deterministic State Machine | Cloud-Local Separation | Proactive Intent |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **DiPECS** | 2024–26 | Android + Linux | Yes (Cloud) | Yes (Air-gap) | Yes (Golden Traces) | Yes (Mechanism-Policy) | Yes |
| AIOS | 2024 | Platform-agnostic | Yes | No | No | No | No |
| OS-Copilot | 2024 | Desktop Linux | Yes (GPT-4) | No | No | No | No |
| UFO/UFO2 | 2024–25 | Windows | Yes (GPT-Vision) | No | No | No | No |
| MemGPT | 2023 | Platform-agnostic | Yes | No | No | No | No |
| FALCON | 2012 | Windows Phone | No (ML) | No | No | No | Yes |
| DeepAPP | 2019 | Android | No (RL) | No | No | No | Yes |
| Atten-Transformer | 2025 | Offline eval only | No (DL) | No | No | No | Yes |
| Apple Intelligence | 2016+ | iOS/macOS | Yes (local + PCC) | Yes (DP + Secure Enclave) | No | Partial (PCC) | Yes |
| Android Standby | 2018+ | Android | No (TFLite) | No | No | No | Partial |
| SeqMF | 2023 | Simulated | No (MF) | Yes (FL) | No | No | Yes |

### 5.2 Benchmark & Evaluation Method Comparison

| System | Benchmark Suite | Key Metrics | Dataset Scale | Reproducible? |
| :--- | :--- | :--- | :--- | :--- |
| AIOS | HumanEval, GAIA, MINT | Success Rate, Throughput, Latency | Synthetic tasks | Yes |
| OS-Copilot | GAIA, SheetCopilot | Success Rate, Pass Rate | Synthetic tasks | Yes |
| UFO | Custom Windows tasks | Success %, Completion %, Steps | 50 tasks, 9 apps | Partially |
| SWE-agent | SWE-bench, HumanEvalFix | Pass@1 | 2,294 GitHub issues | Yes |
| MemGPT | Deep Memory Retrieval | Accuracy, F1 | Custom QA sets | Yes |
| FALCON | Real user traces | Latency (s), Energy (%), Staleness (min) | 16 users | No |
| DeepAPP | Real user traces + field | Precision, Recall, Satisfaction % | 29 users (field) | No |
| Atten-Transformer | Tsinghua + LSapp | HR@K, MRR@K, NDCG@K | 1,000+ users, 4M+ records | Partially |
| GNN+Self-Attention | Tsinghua | HR@K, MRR@K, NDCG@K | 1,000 users, 4M records | Partially |
| Appformer | Shanghai carrier | HR@K, MRR@K, F1 | 1 city, 1 week | No |
| SeqMF | Custom FL simulation | Privacy-utility trade-off | Undisclosed | Partially |
| Apple Intelligence | Internal A/B testing | Undisclosed | 100M+ devices | No |
| Android Standby | Fleet telemetry | Battery savings | Billions of devices | No |

---

## 6. Benchmark Landscape Summary

### 6.1 Common Evaluation Metrics by Domain

**LLM OS / Agent Systems**:

- Success Rate (SR%) on standardized benchmarks (GAIA, SWE-bench, HumanEval)
- Throughput (system calls/sec, tasks/min)
- Latency (agent wait time, end-to-end task time)
- Token consumption (cost efficiency)

**App / Intent Prediction**:

- HR@K (Hit Rate at K) — the dominant metric
- MRR@K (Mean Reciprocal Rank)
- NDCG@K (Normalized Discounted Cumulative Gain)
- Precision and Recall (older works)
- F1-score (macro/micro)

**Proactive Systems**:

- Latency savings (seconds reduced per interaction)
- Energy overhead (% battery per day)
- Content staleness (minutes)
- User satisfaction scores (field studies)

**Privacy**:

- Privacy-utility trade-off curves
- Differential privacy epsilon (ε) values (rarely reported)
- Accuracy loss from privacy mechanisms (%)
- F1 under obfuscation

### 6.2 Standard Datasets

| Dataset | Domain | Scale | Availability |
| :--- | :--- | :--- | :--- |
| Tsinghua App Usage | App prediction | 1,000 users, 4.17M records, 7 days | Academic |
| LSapp | App prediction | 292 users, 87 apps, 600K records | Academic |
| SWE-bench | Code repair | 2,294 real GitHub issues | Public |
| GAIA | General agent tasks | 466 tasks, 3 difficulty levels | Public |
| HumanEval / HumanEvalFix | Code generation/repair | 164 problems | Public |
| Shanghai Carrier | App prediction + POI | 1 city, 1 week | Non-public |

---

## 7. Differentiation of DiPECS

Based on this survey, DiPECS occupies a unique position at the intersection of several research threads. Key differentiators:

1. **Deterministic State Machine with Replay**: No existing LLM-OS or intent prediction system enforces fully observable, replayable state transitions. Golden trace validation (`data/traces/`) is architecturally unique. The closest analog is formal verification in traditional OS research, not in any AI-OS system.

2. **Privacy Air-Gap as Architecture, Not Add-On**: Apple's Private Cloud Compute is the closest analog, but it operates as infrastructure (hardware enclaves + cryptographic routing), not as an explicit software architectural layer. DiPECS's `aios-core` privacy scrubbing before any cloud transmission is a distinct pattern — comparable to a data diode in classified systems.

3. **Mechanism-Policy Separation**: While cloud-edge computing papers discuss workload partitioning, none frame it as an explicit mechanism (local, deterministic) vs. policy (cloud, stochastic) separation. This maps to the Lampson/Levin principle from classic OS theory, applied to LLM-era systems.

4. **Android-First with Cross-Compile**: All LLM-OS works target desktop or platform-agnostic backends. App prediction works evaluate on Android traces but do not build OS-level infrastructure. DiPECS targets Android API 33 with NDK cross-compilation as a first-class concern.

5. **LLM for Prediction, Not Interaction**: Existing LLM-OS systems use LLMs to interpret and execute user commands (task automation). DiPECS uses the cloud LLM for predictive intent inference — acting *before* explicit user request — which aligns more with the anticipatory computing tradition (FALCON, DeepAPP) but using modern LLM capabilities.

### Suggested Benchmark Strategy for DiPECS

Based on the evaluation landscape above, a comprehensive DiPECS evaluation should cover:

| Dimension | Metric | Precedent |
| :--- | :--- | :--- |
| Intent prediction accuracy | HR@1, HR@5, MRR@5 | Atten-Transformer, GNN+SA |
| Prediction latency | End-to-end ms (local + cloud round-trip) | FALCON, DeepAPP |
| Privacy overhead | Scrubbing latency (μs), data size reduction (%) | Novel |
| State machine correctness | Golden trace pass rate (%) | Novel |
| Energy impact | % battery/day for prediction pipeline | FALCON, Android Standby |
| Determinism | Replay divergence rate (should be 0%) | Novel |
| Cloud dependency | Graceful degradation under offline/high-latency conditions | OfflineAdapter replay |

---

## 8. References

1. Mei, K. et al. "AIOS: LLM Agent Operating System." arXiv:2403.16971, 2024.
2. Wu, Z. et al. "OS-Copilot: Towards Generalist Computer Agents with Self-Improvement." arXiv:2402.07456, 2024.
3. Zhang, C. et al. "UFO: A UI-Focused Agent for Windows OS Interaction." arXiv:2402.07939, 2024.
4. Zhang, C. et al. "UFO2: The Desktop AgentOS." arXiv:2504.14603, 2025.
5. Yang, J. et al. "SWE-agent: Agent-Computer Interfaces Enable Automated Software Engineering." NeurIPS, 2024.
6. Packer, C. et al. "MemGPT: Towards LLMs as Operating Systems." arXiv:2310.08560, 2023.
7. Ge, Y. et al. "LLM as OS, Agents as Apps." arXiv:2312.03815, 2023.
8. Yan, T. et al. "Fast App Launching for Mobile Devices Using Predictive User Context." ACM MobiSys, 2012.
9. Zhao, S. et al. "AppUsage2Vec: Modeling Smartphone App Usage for Prediction." IEEE ICDE, 2019.
10. Shen, Z. et al. "DeepAPP: A Deep Reinforcement Learning Framework for Mobile Application Usage Prediction." ACM SenSys, 2019.
11. Li, L. et al. "Atten-Transformer: A Deep Learning Framework for User App Usage Prediction." arXiv:2502.16957, 2025.
12. Chen, J. et al. "Next App Prediction Based on Graph Neural Networks and Self-Attention Enhancement." Scientific Reports, 2025.
13. Sun, C. et al. "Appformer: Mobile App Usage Prediction via Progressive Multi-Modal Data Fusion." arXiv:2407.19414, 2024.
14. Pejovic, V. & Musolesi, M. "Anticipatory Mobile Computing: A Survey." ACM Computing Surveys 47(3), 2015.
15. Sayapin, A. et al. "SeqMF: Federated Privacy-Preserving Collaborative Filtering for On-Device Next App Prediction." arXiv:2303.04744, 2023.
16. Brandao, A. et al. "Prediction of Mobile App Privacy Preferences with User Profiles via Federated Learning." ACM CODASPY, 2022.
17. Trakadas, P. et al. "RAMOS: A Reference Architecture for Cloud-Edge Meta-Operating Systems." Sensors, 2022.
18. Liu, J. et al. "Edge-Cloud Collaborative Computing on Distributed Intelligence and Model Optimization: A Survey." arXiv:2505.01821, 2025.
