//! ZLE miscellaneous operations
//!
//! Direct port from zsh/Src/Zle/zle_misc.c

use super::main::Zle;

impl Zle {
    /// Exchange point and mark
    pub fn exchange_point_and_mark(&mut self) {
        std::mem::swap(&mut self.zlecs, &mut self.mark);
        self.resetneeded = true;
    }
    
    /// Set mark at current position
    pub fn set_mark_here(&mut self) {
        self.mark = self.zlecs;
    }
    
    /// Copy region as kill
    pub fn copy_region_as_kill(&mut self) {
        let (start, end) = if self.zlecs < self.mark {
            (self.zlecs, self.mark)
        } else {
            (self.mark, self.zlecs)
        };
        
        let text: Vec<char> = self.zleline[start..end].to_vec();
        self.killring.push_front(text);
        if self.killring.len() > self.killringmax {
            self.killring.pop_back();
        }
    }
    
    /// Kill region (between point and mark)
    pub fn kill_region(&mut self) {
        let (start, end) = if self.zlecs < self.mark {
            (self.zlecs, self.mark)
        } else {
            (self.mark, self.zlecs)
        };
        
        let text: Vec<char> = self.zleline.drain(start..end).collect();
        self.killring.push_front(text);
        if self.killring.len() > self.killringmax {
            self.killring.pop_back();
        }
        
        self.zlell -= end - start;
        self.zlecs = start;
        self.mark = start;
        self.resetneeded = true;
    }
    
    /// Capitalize word
    pub fn capitalize_word(&mut self) {
        // Find word start
        while self.zlecs < self.zlell && !self.zleline[self.zlecs].is_alphanumeric() {
            self.zlecs += 1;
        }
        
        // Capitalize first letter
        if self.zlecs < self.zlell && self.zleline[self.zlecs].is_alphabetic() {
            self.zleline[self.zlecs] = self.zleline[self.zlecs].to_uppercase().next().unwrap_or(self.zleline[self.zlecs]);
            self.zlecs += 1;
        }
        
        // Lowercase rest of word
        while self.zlecs < self.zlell && self.zleline[self.zlecs].is_alphanumeric() {
            self.zleline[self.zlecs] = self.zleline[self.zlecs].to_lowercase().next().unwrap_or(self.zleline[self.zlecs]);
            self.zlecs += 1;
        }
        
        self.resetneeded = true;
    }
    
    /// Downcase word
    pub fn downcase_word(&mut self) {
        // Find word start
        while self.zlecs < self.zlell && !self.zleline[self.zlecs].is_alphanumeric() {
            self.zlecs += 1;
        }
        
        // Lowercase word
        while self.zlecs < self.zlell && self.zleline[self.zlecs].is_alphanumeric() {
            self.zleline[self.zlecs] = self.zleline[self.zlecs].to_lowercase().next().unwrap_or(self.zleline[self.zlecs]);
            self.zlecs += 1;
        }
        
        self.resetneeded = true;
    }
    
    /// Upcase word
    pub fn upcase_word(&mut self) {
        // Find word start
        while self.zlecs < self.zlell && !self.zleline[self.zlecs].is_alphanumeric() {
            self.zlecs += 1;
        }
        
        // Uppercase word
        while self.zlecs < self.zlell && self.zleline[self.zlecs].is_alphanumeric() {
            self.zleline[self.zlecs] = self.zleline[self.zlecs].to_uppercase().next().unwrap_or(self.zleline[self.zlecs]);
            self.zlecs += 1;
        }
        
        self.resetneeded = true;
    }
    
    /// Transpose words
    pub fn transpose_words(&mut self) {
        // TODO: implement transpose words
    }
    
    /// Quote line
    pub fn quote_line(&mut self) {
        // Insert single quotes around line
        self.zleline.insert(0, '\'');
        self.zlell += 1;
        self.zlecs += 1;
        self.zleline.push('\'');
        self.zlell += 1;
        self.resetneeded = true;
    }
    
    /// Quote region
    pub fn quote_region(&mut self) {
        let (start, end) = if self.zlecs < self.mark {
            (self.zlecs, self.mark)
        } else {
            (self.mark, self.zlecs)
        };
        
        self.zleline.insert(end, '\'');
        self.zleline.insert(start, '\'');
        self.zlell += 2;
        self.zlecs = end + 2;
        self.mark = start;
        self.resetneeded = true;
    }
}
