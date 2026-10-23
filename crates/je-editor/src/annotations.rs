//! Porte de `src/vs/editor/common/model/tokens/annotations.ts`: `IAnnotation`, `IAnnotatedString`,
//! `AnnotatedString`, `IAnnotationUpdate`, `ISerializedAnnotation` e `AnnotationsUpdate`.
//!
//! Representação:
//! - `IAnnotation<T>` é [`Annotation`]. `IAnnotationUpdate<T>` (`{ range, annotation: T | undefined }`) é
//!   estruturalmente um `IAnnotation<T | undefined>`, então é o apelido [`AnnotationUpdate`]. Assim o
//!   `rebase` do original (que cria um `AnnotatedString<T | undefined>` sobre as mesmas anotações) não
//!   precisa de conversão.
//! - `IAnnotatedString<T>` é o trait [`IAnnotatedString`]. O `clone()` dele é `clone_annotated_string`, que
//!   devolve o objeto em caixa; [`AnnotatedString`] também implementa `Clone` (a mesma cópia rasa).
//! - O `slice` e o `splice` do JavaScript aceitam índices negativos (contam do fim). O índice inicial de
//!   uma anotação vazia na posição 0 pode virar `-1`, então os dois são reproduzidos por [`js_splice`] e
//!   por `js_array_slice`.
//! - `DefinedValue` (`object | string | number | boolean`) é o limite `Truthy` em `serialize` e
//!   `deserialize`, porque o original testa `!annotation.annotation` (o "falso" do JavaScript).
//! - Offsets são `i32` em unidades UTF-16, como no [`OffsetRange`].

use std::collections::VecDeque;

use je_base::arrays::binary_search2;
use je_base::dynamic::Truthy;

use crate::edits::edit::BaseEdit;
use crate::edits::string_edit::StringEdit;
use crate::offset_range::{OffsetRange, js_array_slice};

/// `IAnnotation<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Annotation<T> {
    pub range: OffsetRange,
    pub annotation: T,
}

/// `IAnnotationUpdate<T>`: `annotation` `None` (o `undefined` do original) remove a anotação do intervalo.
pub type AnnotationUpdate<T> = Annotation<Option<T>>;

/// `IAnnotatedString<T>`.
pub trait IAnnotatedString<T: Clone> {
    /// Define as anotações de um intervalo. As anotações devem estar ordenadas e sem sobreposição.
    fn set_annotations(&mut self, annotations: &AnnotationsUpdate<T>);
    /// As anotações que cruzam o intervalo dado.
    fn get_annotations_intersecting(&self, range: OffsetRange) -> Vec<Annotation<T>>;
    /// Todas as anotações. O método é usado em teste.
    fn get_all_annotations(&self) -> Vec<Annotation<T>>;
    /// Aplica uma edição de string. Devolve as anotações apagadas (que ficaram vazias) pela edição.
    fn apply_edit(&mut self, edit: &StringEdit) -> Vec<Annotation<T>>;
    /// `clone()`.
    fn clone_annotated_string(&self) -> Box<dyn IAnnotatedString<T>>;
}

/// `Array.prototype.splice(start, deleteCount, ...items)`: `start` negativo conta do fim e os dois são
/// limitados ao tamanho.
fn js_splice<T>(array: &mut Vec<T>, start: i64, delete_count: i64, insert: Option<T>) {
    let len = array.len() as i64;
    let actual_start = if start < 0 { (len + start).max(0) } else { start.min(len) };
    let actual_delete = delete_count.max(0).min(len - actual_start);
    let from = actual_start as usize;
    let to = (actual_start + actual_delete) as usize;
    array.splice(from..to, insert);
}

/// `AnnotatedString<T>`.
#[derive(Clone, Debug)]
pub struct AnnotatedString<T: Clone> {
    /// Anotações não se cruzam e são contíguas no array.
    annotations: Vec<Annotation<T>>,
}

impl<T: Clone> Default for AnnotatedString<T> {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl<T: Clone> AnnotatedString<T> {
    pub fn new(annotations: Vec<Annotation<T>>) -> Self {
        Self { annotations }
    }

    /// `this._annotations[index]?.range`: o índice fora do array (inclusive negativo) dá `None`.
    fn range_at(&self, index: i64) -> Option<OffsetRange> {
        if index < 0 {
            return None;
        }
        self.annotations.get(index as usize).map(|a| a.range)
    }

    /// Define as anotações de um intervalo.
    /// As anotações devem estar ordenadas e sem sobreposição.
    /// Se o valor da anotação é `None`, a anotação é removida.
    pub fn set_annotations(&mut self, annotations: &AnnotationsUpdate<T>) {
        for annotation in annotations.annotations() {
            let start_index = self.get_start_index_of_intersecting_annotation(annotation.range.start);
            let end_index_exclusive = self.get_end_index_of_intersecting_annotation(annotation.range.end_exclusive);
            let delete_count = end_index_exclusive - start_index;
            match &annotation.annotation {
                Some(value) => js_splice(
                    &mut self.annotations,
                    start_index,
                    delete_count,
                    Some(Annotation { range: annotation.range, annotation: value.clone() }),
                ),
                None => js_splice(&mut self.annotations, start_index, delete_count, None),
            }
        }
    }

    /// Devolve todas as anotações que cruzam o intervalo dado.
    pub fn get_annotations_intersecting(&self, range: OffsetRange) -> Vec<Annotation<T>> {
        let start_index = self.get_start_index_of_intersecting_annotation(range.start);
        let end_index_exclusive = self.get_end_index_of_intersecting_annotation(range.end_exclusive);
        js_array_slice(&self.annotations, start_index as i32, Some(end_index_exclusive as i32))
    }

    fn get_start_index_of_intersecting_annotation(&self, offset: i32) -> i64 {
        // Acha o índice à esquerda do offset
        let start_index_where_to_replace =
            binary_search2(self.annotations.len(), |index| f64::from(self.annotations[index].range.start - offset));
        if start_index_where_to_replace >= 0 {
            let mut start_index = start_index_where_to_replace;
            // Inclui também a anotação seguinte se ela termina exatamente no offset (fronteira que toca)
            if let Some(next_candidate) = self.range_at(start_index)
                && next_candidate.end_exclusive == offset
            {
                start_index -= 1;
            }
            start_index
        } else {
            let candidate_index = -(start_index_where_to_replace + 2);
            match self.range_at(candidate_index) {
                Some(candidate) if offset >= candidate.start && offset < candidate.end_exclusive => candidate_index,
                _ => -(start_index_where_to_replace + 1),
            }
        }
    }

    fn get_end_index_of_intersecting_annotation(&self, offset: i32) -> i64 {
        // Acha o índice à direita do offset
        let end_index_where_to_replace =
            binary_search2(self.annotations.len(), |index| f64::from(self.annotations[index].range.end_exclusive - offset));
        if end_index_where_to_replace >= 0 {
            let mut end_index_exclusive = end_index_where_to_replace + 1;
            // Inclui também a anotação seguinte se ela começa exatamente no offset (fronteira que toca)
            if let Some(next_candidate) = self.range_at(end_index_exclusive)
                && next_candidate.start == offset
            {
                end_index_exclusive += 1;
            }
            end_index_exclusive
        } else {
            match self.range_at(-(end_index_where_to_replace + 1)) {
                Some(candidate) if offset >= candidate.start && offset <= candidate.end_exclusive => {
                    -end_index_where_to_replace
                }
                _ => -(end_index_where_to_replace + 1),
            }
        }
    }

    /// Devolve uma cópia de todas as anotações.
    pub fn get_all_annotations(&self) -> Vec<Annotation<T>> {
        self.annotations.clone()
    }

    /// Aplica uma edição de string à string anotada, atualizando os intervalos das anotações.
    /// Devolve as anotações apagadas (que ficaram vazias) pela edição.
    pub fn apply_edit(&mut self, edit: &StringEdit) -> Vec<Annotation<T>> {
        let mut annotations: VecDeque<Annotation<T>> = self.annotations.iter().cloned().collect();

        // trata as edições como a remoção do intervalo substituído e depois uma inserção que estende o primeiro intervalo
        let mut final_annotations: Vec<Annotation<T>> = Vec::new();
        let mut deleted_annotations: Vec<Annotation<T>> = Vec::new();

        let mut offset = 0;

        for e in edit.replacements() {
            let new_len = e.new_text.len() as i32;

            // anotações antes da edição atual
            while let Some(front) = annotations.front() {
                let range = front.range;
                if range.end_exclusive >= e.replace_range.start {
                    break;
                }
                let annotation = annotations.pop_front().expect("há uma anotação na frente");
                let new_annotation = Annotation { range: range.delta(offset), annotation: annotation.annotation };
                if !new_annotation.range.is_empty() {
                    final_annotations.push(new_annotation);
                } else {
                    deleted_annotations.push(new_annotation);
                }
            }

            let mut intersecting: Vec<Annotation<T>> = Vec::new();
            while let Some(front) = annotations.front() {
                if !front.range.intersects_or_touches(e.replace_range) {
                    break;
                }
                intersecting.push(annotations.pop_front().expect("há uma anotação na frente"));
            }

            for (i, annotation) in intersecting.into_iter().enumerate().rev() {
                let mut r = annotation.range;

                // O texto inserido estende a primeira anotação que cruza, se a edição de fato a sobrepõe
                let should_extend =
                    i == 0 && (e.replace_range.end_exclusive > r.start) && (e.replace_range.start < r.end_exclusive);
                // A anotação encolhe pela sobreposição e depois cresce com o comprimento do texto novo
                let overlap = r.intersect(e.replace_range).expect("o intervalo cruza ou toca a substituição").length();
                r = r.delta_end(-overlap + if should_extend { new_len } else { 0 });

                // Se a anotação começa depois do início da edição, desloca para a esquerda até ele
                let range_ahead_of_replace_range = r.start - e.replace_range.start;
                if range_ahead_of_replace_range > 0 {
                    r = r.delta(-range_ahead_of_replace_range);
                }

                // Se a anotação não deve ser estendida E está depois ou no início da edição, move-a para depois do texto inserido
                if !should_extend && range_ahead_of_replace_range >= 0 {
                    r = r.delta(new_len);
                }

                // Já levamos o nosso deslocamento em conta.
                // Como devolvemos r à fila (que soma o deslocamento de novo),
                // temos de tirá-lo aqui.
                r = r.delta(-(new_len - e.replace_range.length()));

                annotations.push_front(Annotation { annotation: annotation.annotation, range: r });
            }

            offset += new_len - e.replace_range.length();
        }

        while let Some(annotation) = annotations.pop_front() {
            let new_annotation = Annotation { annotation: annotation.annotation, range: annotation.range.delta(offset) };
            if !new_annotation.range.is_empty() {
                final_annotations.push(new_annotation);
            } else {
                deleted_annotations.push(new_annotation);
            }
        }
        self.annotations = final_annotations;
        deleted_annotations
    }

    /// Cria uma cópia rasa desta string anotada.
    pub fn clone_shallow(&self) -> AnnotatedString<T> {
        AnnotatedString::new(self.annotations.clone())
    }
}

impl<T: Clone + 'static> IAnnotatedString<T> for AnnotatedString<T> {
    fn set_annotations(&mut self, annotations: &AnnotationsUpdate<T>) {
        AnnotatedString::set_annotations(self, annotations);
    }

    fn get_annotations_intersecting(&self, range: OffsetRange) -> Vec<Annotation<T>> {
        AnnotatedString::get_annotations_intersecting(self, range)
    }

    fn get_all_annotations(&self) -> Vec<Annotation<T>> {
        AnnotatedString::get_all_annotations(self)
    }

    fn apply_edit(&mut self, edit: &StringEdit) -> Vec<Annotation<T>> {
        AnnotatedString::apply_edit(self, edit)
    }

    fn clone_annotated_string(&self) -> Box<dyn IAnnotatedString<T>> {
        Box::new(self.clone_shallow())
    }
}

/// `ISerializedAnnotation<TSerializedProperty>`.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedAnnotation<P> {
    pub range: SerializedRange,
    pub annotation: Option<P>,
}

/// O `{ start: number; endExclusive: number }` de `ISerializedAnnotation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerializedRange {
    pub start: i32,
    pub end_exclusive: i32,
}

/// `AnnotationsUpdate<T>`.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationsUpdate<T: Clone> {
    annotations: Vec<AnnotationUpdate<T>>,
}

impl<T: Clone> AnnotationsUpdate<T> {
    pub fn create(annotations: Vec<AnnotationUpdate<T>>) -> AnnotationsUpdate<T> {
        AnnotationsUpdate { annotations }
    }

    pub fn annotations(&self) -> &[AnnotationUpdate<T>] {
        &self.annotations
    }

    pub fn rebase(&mut self, edit: &StringEdit) {
        let mut annotated_string = AnnotatedString::new(std::mem::take(&mut self.annotations));
        annotated_string.apply_edit(edit);
        self.annotations = annotated_string.get_all_annotations();
    }

    /// `serialize`: `T: Truthy` porque o original testa `!annotation.annotation` (o "falso" do JavaScript,
    /// que descarta também `""`, `0` e `false`, não só `undefined`).
    pub fn serialize<P>(&self, serializing_func: impl Fn(&T) -> P) -> Vec<SerializedAnnotation<P>>
    where
        T: Truthy,
    {
        self.annotations
            .iter()
            .map(|annotation| {
                let range =
                    SerializedRange { start: annotation.range.start, end_exclusive: annotation.range.end_exclusive };
                match &annotation.annotation {
                    Some(value) if value.is_truthy() => {
                        SerializedAnnotation { range, annotation: Some(serializing_func(value)) }
                    }
                    _ => SerializedAnnotation { range, annotation: None },
                }
            })
            .collect()
    }

    pub fn deserialize<P: Truthy>(
        serialized_annotations: &[SerializedAnnotation<P>],
        deserializing_func: impl Fn(&P) -> T,
    ) -> AnnotationsUpdate<T> {
        let annotations = serialized_annotations
            .iter()
            .map(|serialized| {
                let range = OffsetRange::new(serialized.range.start, serialized.range.end_exclusive);
                match &serialized.annotation {
                    Some(value) if value.is_truthy() => {
                        Annotation { range, annotation: Some(deserializing_func(value)) }
                    }
                    _ => Annotation { range, annotation: None },
                }
            })
            .collect();
        AnnotationsUpdate { annotations }
    }
}
