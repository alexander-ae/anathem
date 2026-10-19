use anathem::fusion::rrf;

#[test]
fn sin_ranking_vectorial_conserva_el_de_bm25() {
    assert_eq!(rrf(&[10, 20, 30], &[], 0.25), vec![10, 20, 30]);
}

#[test]
fn sin_ranking_bm25_conserva_el_vectorial() {
    assert_eq!(rrf(&[], &[30, 20, 10], 0.25), vec![30, 20, 10]);
}

#[test]
fn con_peso_bajo_el_vectorial_le_gana_a_bm25() {
    // Cada uno aporta un documento distinto en la posición 1. Con peso 0,25
    // el de bm25 vale la cuarta parte, así que manda el vectorial.
    assert_eq!(rrf(&[10], &[20], 0.25), vec![20, 10]);
}

#[test]
fn con_peso_uno_empatan_y_desempata_el_id() {
    // Mismo caso, pesos iguales: los puntajes son idénticos y el desempate
    // estable por id deja primero al menor. Sin ese desempate el orden
    // dependería del recorrido del HashMap y variaría entre corridas.
    assert_eq!(rrf(&[10], &[20], 1.0), vec![10, 20]);
}

#[test]
fn aparecer_en_los_dos_rankings_pesa_mas_que_liderar_uno() {
    // 20 está en ambos (posición 2 en bm25, 1 en vectorial); 30 solo en el
    // vectorial en posición 2; 10 solo en bm25 en posición 1.
    assert_eq!(rrf(&[10, 20], &[20, 30], 0.25), vec![20, 30, 10]);
}

#[test]
fn no_repite_documentos() {
    let r = rrf(&[1, 2, 3], &[3, 2, 1], 0.5);
    let mut u = r.clone();
    u.sort_unstable();
    u.dedup();
    assert_eq!(r.len(), u.len(), "hay duplicados en {r:?}");
    assert_eq!(u, vec![1, 2, 3]);
}

#[test]
fn desempate_por_id_es_determinista() {
    // 10 en posición 0 de bm25, posición 1 de vectorial
    // 20 en posición 1 de bm25, posición 0 de vectorial
    // Con peso_bm25=1.0 ambos tienen el mismo puntaje total.
    // El desempate estable por id debe poner 10 primero, siempre.
    let expected = vec![10, 20];
    for _ in 0..20 {
        assert_eq!(rrf(&[10, 20], &[20, 10], 1.0), expected);
    }
}

#[test]
fn peso_cero_bm25_pone_solo_vectorial_adelante() {
    // Con peso cero, bm25 no contribuye nada. Los documentos que solo están
    // en bm25 (10, 30) quedan detrás de los que están en vectorial (20, 40),
    // y el orden interno del vectorial se preserva.
    assert_eq!(rrf(&[10, 20, 30, 40], &[20, 40], 0.0), vec![20, 40, 10, 30]);
}

#[test]
fn con_ambos_vacios_devuelve_vacio() {
    assert!(rrf(&[], &[], 0.25).is_empty());
}
