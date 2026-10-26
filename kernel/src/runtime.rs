/* ============================================================
 * runtime.rs — fonctions mémoire attendues par le compilateur
 *
 * Un binaire `no_std` lié sans libc n'a pas de `memset` : or
 * rustc en émet l'appel dès qu'un initialiseur de tableau ou une
 * boucle d'effacement peut être abaissé à un remplissage mémoire.
 * Sans définition, l'édition de liens échoue sur un symbole
 * manquant — le noyau compile mais ne se lie pas.
 *
 * Ces fonctions sont volontairement naïves : la vitesse n'entre pas
 * en jeu ici, et une version optimisée (SSE, AVX) n'apporterait rien
 * à un noyau qui n'en appelle presque jamais.
 *
 * Elles sont écrites en accès volatile. Sans cela, rustc reconnaît
 * la boucle d'octets et la remplace par un appel à `memset`… c'est-à-dire
 * par elle-même : le lien se ferait, mais l'appel bouclerait à
 * l'exécution.
 *
 * Les signatures utilisent `c_void` : c'est celle que rustc attend
 * pour ces symboles, et s'en écarter déclenche un avertissement.
 * ============================================================ */

use core::ffi::c_void;

/// Remplit `n` octets avec la valeur `c`.
///
/// # Sécurité
///
/// `dest` doit désigner une zone de `n` octets accessibles en
/// écriture, et ne pas déborder de la mémoire mappée.
#[no_mangle]
pub unsafe extern "C" fn memset(dest: *mut c_void, c: i32, n: usize) -> *mut c_void {
    let mut at = dest as *mut u8;
    let mut left = n;

    while left > 0 {
        core::ptr::write_volatile(at, c as u8);
        at = at.add(1);
        left -= 1;
    }

    dest
}

/// Copie `n` octets de `src` vers `dest`, zones non chevauchantes.
///
/// # Sécurité
///
/// Comme [`memset`], et les deux zones ne doivent pas se chevaucher.
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let mut d = dest as *mut u8;
    let mut s = src as *const u8;
    let mut left = n;

    while left > 0 {
        core::ptr::write_volatile(d, core::ptr::read_volatile(s));
        d = d.add(1);
        s = s.add(1);
        left -= 1;
    }

    dest
}

/// Copie `n` octets de `src` vers `dest`, zones pouvant se chevaucher.
///
/// Le sens de la copie dépend des adresses : avant si `dest` est
/// avant `src`, arrière sinon. Copier dans le mauvais sens écrase les
/// octets pas encore lus.
///
/// # Sécurité
///
/// Comme [`memcpy`], mais le chevauchement est autorisé.
#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut c_void, src: *const c_void, n: usize) -> *mut c_void {
    let d = dest as *mut u8;
    let s = src as *const u8;

    if (d as usize) == (s as usize) || n == 0 {
        return dest;
    }

    if (d as usize) < (s as usize) {
        let mut i = 0;
        while i < n {
            core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            core::ptr::write_volatile(d.add(i), core::ptr::read_volatile(s.add(i)));
        }
    }

    dest
}

/// Compare `n` octets de `a` et `b`.
///
/// Renvoie un négatif si `a` précède `b`, un positif si `a` suit `b`,
/// zéro si les zones sont égales.
///
/// # Sécurité
///
/// `a` et `b` doivent désigner chacun `n` octets lisibles.
#[no_mangle]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let a = a as *const u8;
    let b = b as *const u8;
    let mut i = 0;

    while i < n {
        let x = core::ptr::read_volatile(a.add(i));
        let y = core::ptr::read_volatile(b.add(i));

        if x != y {
            return x as i32 - y as i32;
        }

        i += 1;
    }

    0
}
