# Plain lowercase Italian; of the one-letter words, a e i o è.
$1 ~ /^[a-zàèéìíòóùú]+$/ && (length($1) > 1 || $1 ~ /^(a|e|i|o|è)$/) { n++; word[n] = $1; count[$1] = $2 }
END {
    # A word whose last vowel lost its accent (perche, piu, cosi) and is
    # less than twice as common as the accented one is that word mistyped:
    # its count goes to the accented spelling. A real pair (e/è, si/sì, la/là)
    # has the plain word far more common, and keeps both.
    for (i = 1; i <= n; i++) {
        a = word[i]
        if (a !~ /[àèéìíòóùú]$/) continue
        last = substr(a, length(a)); gsub(/[àá]/, "a", last); gsub(/[èé]/, "e", last)
        gsub(/[ìí]/, "i", last); gsub(/[òó]/, "o", last); gsub(/[ùú]/, "u", last)
        p = substr(a, 1, length(a) - 1) last
        if ((p in count) && count[p] < 2 * count[a] && (!(p in best) || count[a] > count[best[p]])) best[p] = a
    }
    for (p in best) { count[best[p]] += count[p]; delete count[p] }
    for (i = 1; i <= n; i++) if (word[i] in count) print word[i], count[word[i]]
}
