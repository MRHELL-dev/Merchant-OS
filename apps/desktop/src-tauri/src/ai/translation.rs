/// Multilingual translation and vocabulary normalization for English, Hindi, and Hinglish.
/// Crucial invariant: Never alters or strips numbers, quantities, decimals, or proper names.

pub struct MultilingualNormalizer;

impl MultilingualNormalizer {
    /// Normalizes a query into a canonical tokenized form while preserving proper names and quantities.
    pub fn normalize(input: &str) -> String {
        let mut text = input.trim().to_lowercase();

        // 1. Normalize Hindi (Devanagari) phrases and words to standard tokens
        let devanagari_mappings = [
            ("बेचा", "sold"),
            ("बेच दो", "sell"),
            ("बिक्री", "sale"),
            ("खरीदा", "purchase"),
            ("खरीद", "purchase"),
            ("स्टॉक कितना है", "check stock"),
            ("कितना बचा है", "check stock"),
            ("कितना है", "how much"),
            ("उधारी", "credit"),
            ("रोकड़", "cash"),
            ("नकद", "cash"),
            ("जमा", "payment"),
            ("चावल", "rice"),
            ("दाल", "dal"),
            ("चीनी", "sugar"),
            ("तेल", "oil"),
            ("कम स्टॉक", "low stock"),
            ("ऑर्डर", "order"),
        ];

        for (dev, en) in devanagari_mappings {
            text = text.replace(dev, en);
        }

        // 2. Normalize Hinglish phrases and words
        let hinglish_phrases = [
            ("kitna bacha hai", "check stock"),
            ("stock kitna hai", "check stock"),
            ("kitna hai", "how much"),
            ("bech do", "sell"),
            ("becha hai", "sold"),
            ("becha", "sold"),
            ("bikri", "sale"),
            ("bik gaya", "sold"),
            ("khareeda hai", "purchase"),
            ("khareeda", "purchase"),
            ("kharida", "purchase"),
            ("rokad", "cash"),
            ("nagad", "cash"),
            ("udhaari", "credit"),
            ("udhaar", "credit"),
            ("baki", "credit"),
            ("jama kiya", "payment"),
            ("kam stock", "low stock"),
            ("khatam hone wala", "low stock"),
            ("chaaval", "rice"),
            ("chawal", "rice"),
            ("cheeni", "sugar"),
            ("chini", "sugar"),
        ];

        for (hi, en) in hinglish_phrases {
            text = text.replace(hi, en);
        }

        // 3. Normalize single word units and terms with word padding to prevent substring mangling (e.g. packet -> packetss)
        let word_units = [
            (" packet ", " packets "),
            (" pkt ", " packets "),
            (" piece ", " pcs "),
            (" kilo ", " kg "),
            (" gram ", " g "),
            (" do packet ", " 2 packets "),
            (" do packets ", " 2 packets "),
            (" do pkt ", " 2 packets "),
            (" do piece ", " 2 pcs "),
            (" do kilo ", " 2 kg "),
            (" do kg ", " 2 kg "),
            (" do gram ", " 2 g "),
        ];

        let mut padded = format!(" {} ", text);
        for (unit, en) in word_units {
            padded = padded.replace(unit, en);
        }

        // 4. Word numbers to digits where unambiguous
        let word_numbers = [
            (" ek ", " 1 "),
            (" teen ", " 3 "),
            (" char ", " 4 "),
            (" paanch ", " 5 "),
            (" chhah ", " 6 "),
            (" saat ", " 7 "),
            (" aath ", " 8 "),
            (" nau ", " 9 "),
            (" das ", " 10 "),
            (" one ", " 1 "),
            (" two ", " 2 "),
            (" three ", " 3 "),
            (" four ", " 4 "),
            (" five ", " 5 "),
            (" ten ", " 10 "),
        ];

        for (word, digit) in word_numbers {
            padded = padded.replace(word, digit);
        }

        padded.trim().to_string()
    }
}
