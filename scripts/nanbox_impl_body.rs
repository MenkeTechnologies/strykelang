impl PerlValue {
    pub const UNDEF: PerlValue = PerlValue(nanbox::encode_imm_undef());

    #[inline]
    fn from_heap(arc: Arc<HeapObject>) -> PerlValue {
        let ptr = Arc::into_raw(arc);
        PerlValue(nanbox::encode_heap_ptr(ptr))
    }

    #[inline]
    pub(crate) fn heap_arc(&self) -> Arc<HeapObject> {
        debug_assert!(nanbox::is_heap(self.0));
        unsafe {
            let p = nanbox::decode_heap_ptr::<HeapObject>(self.0) as *const HeapObject;
            Arc::increment_strong_count(p);
            Arc::from_raw(p as *mut HeapObject)
        }
    }

    #[inline]
    pub fn integer(n: i64) -> Self {
        if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            PerlValue(nanbox::encode_imm_int32(n as i32))
        } else {
            Self::from_heap(Arc::new(HeapObject::Integer(n)))
        }
    }

    #[inline]
    pub fn float(f: f64) -> Self {
        if nanbox::float_needs_box(f) {
            Self::from_heap(Arc::new(HeapObject::Float(f)))
        } else {
            PerlValue(f.to_bits())
        }
    }

    #[inline]
    pub fn string(s: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::String(s)))
    }

    #[inline]
    pub fn bytes(b: Arc<Vec<u8>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Bytes(b)))
    }

    #[inline]
    pub fn array(v: Vec<PerlValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Array(v)))
    }

    #[inline]
    pub fn hash(h: IndexMap<String, PerlValue>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Hash(h)))
    }

    #[inline]
    pub fn array_ref(a: Arc<RwLock<Vec<PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ArrayRef(a)))
    }

    #[inline]
    pub fn hash_ref(h: Arc<RwLock<IndexMap<String, PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::HashRef(h)))
    }

    #[inline]
    pub fn scalar_ref(r: Arc<RwLock<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ScalarRef(r)))
    }

    #[inline]
    pub fn code_ref(c: Arc<PerlSub>) -> Self {
        Self::from_heap(Arc::new(HeapObject::CodeRef(c)))
    }

    #[inline]
    pub fn regex(
        rx: Arc<crate::perl_regex::PerlCompiledRegex>,
        pattern_src: String,
        flags: String,
    ) -> Self {
        Self::from_heap(Arc::new(HeapObject::Regex(rx, pattern_src, flags)))
    }

    /// Pattern and flag string stored with a compiled regex (for `=~` / [`Op::RegexMatchDyn`]).
    #[inline]
    pub fn regex_src_and_flags(&self) -> Option<(String, String)> {
        self.with_heap(|h| match h {
            HeapObject::Regex(_, pat, fl) => Some((pat.clone(), fl.clone())),
            _ => None,
        })
        .flatten()
    }

    #[inline]
    pub fn blessed(b: Arc<BlessedRef>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Blessed(b)))
    }

    #[inline]
    pub fn io_handle(name: String) -> Self {
        Self::from_heap(Arc::new(HeapObject::IOHandle(name)))
    }

    #[inline]
    pub fn atomic(a: Arc<Mutex<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Atomic(a)))
    }

    #[inline]
    pub fn set(s: Arc<PerlSet>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Set(s)))
    }

    #[inline]
    pub fn channel_tx(tx: Arc<Sender<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelTx(tx)))
    }

    #[inline]
    pub fn channel_rx(rx: Arc<Receiver<PerlValue>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::ChannelRx(rx)))
    }

    #[inline]
    pub fn async_task(t: Arc<PerlAsyncTask>) -> Self {
        Self::from_heap(Arc::new(HeapObject::AsyncTask(t)))
    }

    #[inline]
    pub fn deque(d: Arc<Mutex<VecDeque<PerlValue>>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Deque(d)))
    }

    #[inline]
    pub fn heap(h: Arc<Mutex<PerlHeap>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Heap(h)))
    }

    #[inline]
    pub fn pipeline(p: Arc<Mutex<PipelineInner>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Pipeline(p)))
    }

    #[inline]
    pub fn capture(c: Arc<CaptureResult>) -> Self {
        Self::from_heap(Arc::new(HeapObject::Capture(c)))
    }

    #[inline]
    pub fn ppool(p: PerlPpool) -> Self {
        Self::from_heap(Arc::new(HeapObject::Ppool(p)))
    }

    #[inline]
    pub fn barrier(b: PerlBarrier) -> Self {
        Self::from_heap(Arc::new(HeapObject::Barrier(b)))
    }

    #[inline]
    pub fn sqlite_conn(c: Arc<Mutex<rusqlite::Connection>>) -> Self {
        Self::from_heap(Arc::new(HeapObject::SqliteConn(c)))
    }

    #[inline]
    pub fn struct_inst(s: Arc<StructInstance>) -> Self {
        Self::from_heap(Arc::new(HeapObject::StructInst(s)))
    }

    /// Heap string payload, if any (allocates).
    #[inline]
    pub fn as_str(&self) -> Option<String> {
        if !nanbox::is_heap(self.0) {
            return None;
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    #[inline]
    pub fn append_to(&self, buf: &mut String) {
        if nanbox::is_imm_undef(self.0) {
            return;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            let mut b = itoa::Buffer::new();
            buf.push_str(b.format(n));
            return;
        }
        if nanbox::is_raw_float_bits(self.0) {
            buf.push_str(&format_float(f64::from_bits(self.0)));
            return;
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => buf.push_str(s),
            HeapObject::Bytes(b) => buf.push_str(&crate::perl_decode::decode_utf8_or_latin1(b)),
            HeapObject::Atomic(arc) => arc.lock().append_to(buf),
            HeapObject::Set(s) => {
                buf.push('{');
                let mut first = true;
                for v in s.values() {
                    if !first {
                        buf.push(',');
                    }
                    first = false;
                    v.append_to(buf);
                }
                buf.push('}');
            }
            HeapObject::ChannelTx(_) => buf.push_str("PCHANNEL::Tx"),
            HeapObject::ChannelRx(_) => buf.push_str("PCHANNEL::Rx"),
            HeapObject::AsyncTask(_) => buf.push_str("AsyncTask"),
            HeapObject::Pipeline(_) => buf.push_str("Pipeline"),
            HeapObject::Capture(_) => buf.push_str("Capture"),
            HeapObject::Ppool(_) => buf.push_str("Ppool"),
            HeapObject::Barrier(_) => buf.push_str("Barrier"),
            HeapObject::SqliteConn(_) => buf.push_str("SqliteConn"),
            HeapObject::StructInst(s) => buf.push_str(&s.def.name),
            _ => buf.push_str(&self.to_string()),
        }
    }

    #[inline]
    pub fn unwrap_atomic(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::Atomic(a) => a.lock().clone(),
            _ => self.clone(),
        }
    }

    #[inline]
    pub fn is_atomic(&self) -> bool {
        if !nanbox::is_heap(self.0) {
            return false;
        }
        let arc = self.heap_arc();
        matches!(&*arc, HeapObject::Atomic(_))
    }

    #[inline]
    pub fn is_true(&self) -> bool {
        if nanbox::is_imm_undef(self.0) {
            return false;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n != 0;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0) != 0.0;
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => !s.is_empty() && s != "0",
            HeapObject::Bytes(b) => !b.is_empty(),
            HeapObject::Array(a) => !a.is_empty(),
            HeapObject::Hash(h) => !h.is_empty(),
            HeapObject::Atomic(arc) => arc.lock().is_true(),
            HeapObject::Set(s) => !s.is_empty(),
            HeapObject::Deque(d) => !d.lock().is_empty(),
            HeapObject::Heap(h) => !h.lock().items.is_empty(),
            HeapObject::Pipeline(_) | HeapObject::Capture(_) => true,
            _ => true,
        }
    }

    #[inline]
    pub fn into_string(self) -> String {
        let bits = self.0;
        std::mem::forget(self);
        if nanbox::is_imm_undef(bits) {
            return String::new();
        }
        if let Some(n) = nanbox::as_imm_int32(bits) {
            let mut buf = itoa::Buffer::new();
            return buf.format(n).to_owned();
        }
        if nanbox::is_raw_float_bits(bits) {
            return format_float(f64::from_bits(bits));
        }
        if nanbox::is_heap(bits) {
            unsafe {
                let arc =
                    Arc::from_raw(nanbox::decode_heap_ptr::<HeapObject>(bits) as *mut HeapObject);
                match Arc::try_unwrap(arc) {
                    Ok(HeapObject::String(s)) => return s,
                    Ok(o) => return PerlValue::from_heap(Arc::new(o)).to_string(),
                    Err(arc) => {
                        return match &*arc {
                            HeapObject::String(s) => s.clone(),
                            _ => PerlValue::from_heap(Arc::clone(&arc)).to_string(),
                        };
                    }
                }
            }
        }
        String::new()
    }

    #[inline]
    pub fn as_str_or_empty(&self) -> String {
        if !nanbox::is_heap(self.0) {
            return String::new();
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => s.clone(),
            _ => String::new(),
        }
    }

    #[inline]
    pub fn to_number(&self) -> f64 {
        if nanbox::is_imm_undef(self.0) {
            return 0.0;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n as f64;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0);
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => parse_number(s),
            HeapObject::Bytes(b) => b.len() as f64,
            HeapObject::Array(a) => a.len() as f64,
            HeapObject::Atomic(arc) => arc.lock().to_number(),
            HeapObject::Set(s) => s.len() as f64,
            HeapObject::ChannelTx(_) | HeapObject::ChannelRx(_) | HeapObject::AsyncTask(_) => 1.0,
            HeapObject::Deque(d) => d.lock().len() as f64,
            HeapObject::Heap(h) => h.lock().items.len() as f64,
            HeapObject::Pipeline(p) => p.lock().source.len() as f64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::Barrier(_)
            | HeapObject::SqliteConn(_)
            | HeapObject::StructInst(_)
            | HeapObject::IOHandle(_) => 1.0,
            _ => 0.0,
        }
    }

    #[inline]
    pub fn to_int(&self) -> i64 {
        if nanbox::is_imm_undef(self.0) {
            return 0;
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return n as i64;
        }
        if nanbox::is_raw_float_bits(self.0) {
            return f64::from_bits(self.0) as i64;
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => parse_number(s) as i64,
            HeapObject::Bytes(b) => b.len() as i64,
            HeapObject::Array(a) => a.len() as i64,
            HeapObject::Atomic(arc) => arc.lock().to_int(),
            HeapObject::Set(s) => s.len() as i64,
            HeapObject::ChannelTx(_) | HeapObject::ChannelRx(_) | HeapObject::AsyncTask(_) => 1,
            HeapObject::Deque(d) => d.lock().len() as i64,
            HeapObject::Heap(h) => h.lock().items.len() as i64,
            HeapObject::Pipeline(p) => p.lock().source.len() as i64,
            HeapObject::Capture(_)
            | HeapObject::Ppool(_)
            | HeapObject::Barrier(_)
            | HeapObject::SqliteConn(_)
            | HeapObject::StructInst(_)
            | HeapObject::IOHandle(_) => 1,
            _ => 0,
        }
    }

    pub fn type_name(&self) -> String {
        if nanbox::is_imm_undef(self.0) {
            return "undef".to_string();
        }
        if nanbox::as_imm_int32(self.0).is_some() {
            return "INTEGER".to_string();
        }
        if nanbox::is_raw_float_bits(self.0) {
            return "FLOAT".to_string();
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(_) => "STRING".to_string(),
            HeapObject::Bytes(_) => "BYTES".to_string(),
            HeapObject::Array(_) => "ARRAY".to_string(),
            HeapObject::Hash(_) => "HASH".to_string(),
            HeapObject::ArrayRef(_) => "ARRAY".to_string(),
            HeapObject::HashRef(_) => "HASH".to_string(),
            HeapObject::ScalarRef(_) => "SCALAR".to_string(),
            HeapObject::CodeRef(_) => "CODE".to_string(),
            HeapObject::Regex(_, _, _) => "Regexp".to_string(),
            HeapObject::Blessed(b) => b.class.clone(),
            HeapObject::IOHandle(_) => "GLOB".to_string(),
            HeapObject::Atomic(_) => "ATOMIC".to_string(),
            HeapObject::Set(_) => "Set".to_string(),
            HeapObject::ChannelTx(_) => "PCHANNEL::Tx".to_string(),
            HeapObject::ChannelRx(_) => "PCHANNEL::Rx".to_string(),
            HeapObject::AsyncTask(_) => "ASYNCTASK".to_string(),
            HeapObject::Deque(_) => "Deque".to_string(),
            HeapObject::Heap(_) => "Heap".to_string(),
            HeapObject::Pipeline(_) => "Pipeline".to_string(),
            HeapObject::Capture(_) => "Capture".to_string(),
            HeapObject::Ppool(_) => "Ppool".to_string(),
            HeapObject::Barrier(_) => "Barrier".to_string(),
            HeapObject::SqliteConn(_) => "SqliteConn".to_string(),
            HeapObject::StructInst(s) => s.def.name.to_string(),
            HeapObject::Integer(_) => "INTEGER".to_string(),
            HeapObject::Float(_) => "FLOAT".to_string(),
        }
    }

    pub fn ref_type(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return PerlValue::string(String::new());
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::ArrayRef(_) => PerlValue::string("ARRAY".into()),
            HeapObject::HashRef(_) => PerlValue::string("HASH".into()),
            HeapObject::ScalarRef(_) => PerlValue::string("SCALAR".into()),
            HeapObject::CodeRef(_) => PerlValue::string("CODE".into()),
            HeapObject::Regex(_, _, _) => PerlValue::string("Regexp".into()),
            HeapObject::Atomic(_) => PerlValue::string("ATOMIC".into()),
            HeapObject::Set(_) => PerlValue::string("Set".into()),
            HeapObject::ChannelTx(_) => PerlValue::string("PCHANNEL::Tx".into()),
            HeapObject::ChannelRx(_) => PerlValue::string("PCHANNEL::Rx".into()),
            HeapObject::AsyncTask(_) => PerlValue::string("ASYNCTASK".into()),
            HeapObject::Deque(_) => PerlValue::string("Deque".into()),
            HeapObject::Heap(_) => PerlValue::string("Heap".into()),
            HeapObject::Pipeline(_) => PerlValue::string("Pipeline".into()),
            HeapObject::Capture(_) => PerlValue::string("Capture".into()),
            HeapObject::Ppool(_) => PerlValue::string("Ppool".into()),
            HeapObject::Barrier(_) => PerlValue::string("Barrier".into()),
            HeapObject::SqliteConn(_) => PerlValue::string("SqliteConn".into()),
            HeapObject::StructInst(s) => PerlValue::string(s.def.name.clone()),
            HeapObject::Bytes(_) => PerlValue::string("BYTES".into()),
            HeapObject::Blessed(b) => PerlValue::string(b.class.clone()),
            _ => PerlValue::string(String::new()),
        }
    }

    pub fn num_cmp(&self, other: &PerlValue) -> Ordering {
        let a = self.to_number();
        let b = other.to_number();
        a.partial_cmp(&b).unwrap_or(Ordering::Equal)
    }

    pub fn str_cmp(&self, other: &PerlValue) -> Ordering {
        self.to_string().cmp(&other.to_string())
    }

    pub fn to_list(&self) -> Vec<PerlValue> {
        if nanbox::is_imm_undef(self.0) {
            return vec![];
        }
        if !nanbox::is_heap(self.0) {
            return vec![self.clone()];
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::Array(a) => a.clone(),
            HeapObject::Hash(h) => h
                .iter()
                .flat_map(|(k, v)| vec![PerlValue::string(k.clone()), v.clone()])
                .collect(),
            HeapObject::Atomic(arc) => arc.lock().to_list(),
            HeapObject::Set(s) => s.values().cloned().collect(),
            HeapObject::Deque(d) => d.lock().iter().cloned().collect(),
            _ => vec![self.clone()],
        }
    }

    pub fn scalar_context(&self) -> PerlValue {
        if !nanbox::is_heap(self.0) {
            return self.clone();
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::Array(a) => PerlValue::integer(a.len() as i64),
            HeapObject::Hash(h) => {
                if h.is_empty() {
                    PerlValue::integer(0)
                } else {
                    PerlValue::string(format!("{}/{}", h.len(), h.capacity()))
                }
            }
            HeapObject::Set(s) => PerlValue::integer(s.len() as i64),
            HeapObject::Deque(d) => PerlValue::integer(d.lock().len() as i64),
            HeapObject::Heap(h) => PerlValue::integer(h.lock().items.len() as i64),
            HeapObject::Pipeline(p) => PerlValue::integer(p.lock().source.len() as i64),
            HeapObject::Capture(_) | HeapObject::Ppool(_) | HeapObject::Barrier(_) => {
                PerlValue::integer(1)
            }
            _ => self.clone(),
        }
    }
}

impl fmt::Display for PerlValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if nanbox::is_imm_undef(self.0) {
            return Ok(());
        }
        if let Some(n) = nanbox::as_imm_int32(self.0) {
            return write!(f, "{n}");
        }
        if nanbox::is_raw_float_bits(self.0) {
            return write!(f, "{}", format_float(f64::from_bits(self.0)));
        }
        let arc = self.heap_arc();
        match &*arc {
            HeapObject::String(s) => f.write_str(s),
            HeapObject::Bytes(b) => f.write_str(&crate::perl_decode::decode_utf8_or_latin1(b)),
            HeapObject::Array(a) => {
                for v in a {
                    write!(f, "{v}")?;
                }
                Ok(())
            }
            HeapObject::Hash(h) => write!(f, "{}/{}", h.len(), h.capacity()),
            HeapObject::ArrayRef(_) => f.write_str("ARRAY(0x...)"),
            HeapObject::HashRef(_) => f.write_str("HASH(0x...)"),
            HeapObject::ScalarRef(_) => f.write_str("SCALAR(0x...)"),
            HeapObject::CodeRef(sub) => write!(f, "CODE({})", sub.name),
            HeapObject::Regex(_, src, _) => write!(f, "(?:{src})"),
            HeapObject::Blessed(b) => write!(f, "{}=HASH(0x...)", b.class),
            HeapObject::IOHandle(name) => f.write_str(name),
            HeapObject::Atomic(arc) => write!(f, "{}", arc.lock()),
            HeapObject::Set(s) => {
                f.write_str("{")?;
                if !s.is_empty() {
                    let mut iter = s.values();
                    if let Some(v) = iter.next() {
                        write!(f, "{v}")?;
                    }
                    for v in iter {
                        write!(f, ",{v}")?;
                    }
                }
                f.write_str("}")
            }
            HeapObject::ChannelTx(_) => f.write_str("PCHANNEL::Tx"),
            HeapObject::ChannelRx(_) => f.write_str("PCHANNEL::Rx"),
            HeapObject::AsyncTask(_) => f.write_str("AsyncTask"),
            HeapObject::Deque(d) => write!(f, "Deque({})", d.lock().len()),
            HeapObject::Heap(h) => write!(f, "Heap({})", h.lock().items.len()),
            HeapObject::Pipeline(p) => {
                let g = p.lock();
                write!(f, "Pipeline({} ops)", g.ops.len())
            }
            HeapObject::Capture(c) => write!(f, "Capture(exit={})", c.exitcode),
            HeapObject::Ppool(_) => f.write_str("Ppool"),
            HeapObject::Barrier(_) => f.write_str("Barrier"),
            HeapObject::SqliteConn(_) => f.write_str("SqliteConn"),
            HeapObject::StructInst(s) => write!(f, "{}=STRUCT(...)", s.def.name),
            HeapObject::Integer(n) => write!(f, "{n}"),
            HeapObject::Float(fl) => write!(f, "{}", format_float(*fl)),
        }
    }
}

/// Stable key for set membership (dedup of `PerlValue` in this runtime).
pub fn set_member_key(v: &PerlValue) -> String {
    if nanbox::is_imm_undef(v.0) {
        return "u:".to_string();
    }
    if let Some(n) = nanbox::as_imm_int32(v.0) {
        return format!("i:{n}");
    }
    if nanbox::is_raw_float_bits(v.0) {
        return format!("f:{}", f64::from_bits(v.0).to_bits());
    }
    let arc = v.heap_arc();
    match &*arc {
        HeapObject::String(s) => format!("s:{s}"),
        HeapObject::Bytes(b) => {
            use std::fmt::Write as _;
            let mut h = String::with_capacity(b.len() * 2);
            for &x in b.iter() {
                let _ = write!(&mut h, "{:02x}", x);
            }
            format!("by:{h}")
        }
        HeapObject::Array(a) => {
            let parts: Vec<_> = a.iter().map(set_member_key).collect();
            format!("a:{}", parts.join(","))
        }
        HeapObject::Hash(h) => {
            let mut keys: Vec<_> = h.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(h.get(k).unwrap())))
                .collect();
            format!("h:{}", parts.join(","))
        }
        HeapObject::Set(inner) => {
            let mut keys: Vec<_> = inner.keys().cloned().collect();
            keys.sort();
            format!("S:{}", keys.join(","))
        }
        HeapObject::ArrayRef(a) => {
            let g = a.read();
            let parts: Vec<_> = g.iter().map(set_member_key).collect();
            format!("ar:{}", parts.join(","))
        }
        HeapObject::HashRef(h) => {
            let g = h.read();
            let mut keys: Vec<_> = g.keys().cloned().collect();
            keys.sort();
            let parts: Vec<_> = keys
                .iter()
                .map(|k| format!("{}={}", k, set_member_key(g.get(k).unwrap())))
                .collect();
            format!("hr:{}", parts.join(","))
        }
        HeapObject::Blessed(b) => {
            let d = b.data.read();
            format!("b:{}:{}", b.class, set_member_key(&d))
        }
        HeapObject::ScalarRef(_) => format!("sr:{v}"),
        HeapObject::CodeRef(_) => format!("c:{v}"),
        HeapObject::Regex(_, src, _) => format!("r:{src}"),
        HeapObject::IOHandle(s) => format!("io:{s}"),
        HeapObject::Atomic(arc) => format!("at:{}", set_member_key(&arc.lock())),
        HeapObject::ChannelTx(tx) => format!("chtx:{:p}", Arc::as_ptr(tx)),
        HeapObject::ChannelRx(rx) => format!("chrx:{:p}", Arc::as_ptr(rx)),
        HeapObject::AsyncTask(t) => format!("async:{:p}", Arc::as_ptr(t)),
        HeapObject::Deque(d) => format!("dq:{:p}", Arc::as_ptr(d)),
        HeapObject::Heap(h) => format!("hp:{:p}", Arc::as_ptr(h)),
        HeapObject::Pipeline(p) => format!("pl:{:p}", Arc::as_ptr(p)),
        HeapObject::Capture(c) => format!("cap:{:p}", Arc::as_ptr(c)),
        HeapObject::Ppool(p) => format!("pp:{:p}", Arc::as_ptr(&p.0)),
        HeapObject::Barrier(b) => format!("br:{:p}", Arc::as_ptr(&b.0)),
        HeapObject::SqliteConn(c) => format!("sql:{:p}", Arc::as_ptr(c)),
        HeapObject::StructInst(s) => format!("st:{}:{:?}", s.def.name, s.values),
        HeapObject::Integer(n) => format!("i:{n}"),
        HeapObject::Float(fl) => format!("f:{}", fl.to_bits()),
    }
}

pub fn set_from_elements<I: IntoIterator<Item = PerlValue>>(items: I) -> PerlValue {
    let mut map = PerlSet::new();
    for v in items {
        let k = set_member_key(&v);
        map.insert(k, v);
    }
    PerlValue::set(Arc::new(map))
}

/// Underlying set for union/intersection, including `mysync $s` (`Atomic` wrapping `Set`).
#[inline]
pub fn set_payload(v: &PerlValue) -> Option<Arc<PerlSet>> {
    if !nanbox::is_heap(v.0) {
        return None;
    }
    let arc = v.heap_arc();
    match &*arc {
        HeapObject::Set(s) => Some(Arc::clone(s)),
        HeapObject::Atomic(a) => set_payload(&a.lock()),
        _ => None,
    }
}

pub fn set_union(a: &PerlValue, b: &PerlValue) -> Option<PerlValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = (*ia).clone();
    for (k, v) in ib.iter() {
        m.entry(k.clone()).or_insert_with(|| v.clone());
    }
    Some(PerlValue::set(Arc::new(m)))
}

pub fn set_intersection(a: &PerlValue, b: &PerlValue) -> Option<PerlValue> {
    let ia = set_payload(a)?;
    let ib = set_payload(b)?;
    let mut m = PerlSet::new();
    for (k, v) in ia.iter() {
        if ib.contains_key(k) {
            m.insert(k.clone(), v.clone());
        }
    }
    Some(PerlValue::set(Arc::new(m)))
}
