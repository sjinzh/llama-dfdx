use dfdx::{
    shapes::{HasShape, Shape, Unit},
    tensor::{CopySlice, Cpu, Tensor, TensorFromVec, ZerosTensor},
};

use std::any::{Any, TypeId};

#[derive(Debug)]
pub enum LazyTensor<S: Shape, E: Unit> {
    Disk {
        path: std::path::PathBuf,
        shape: S,
    },
    CPU(Tensor<S, E, Cpu>),
    #[cfg(feature = "cuda")]
    CUDA(Tensor<S, E, dfdx::tensor::Cuda>),
}

impl<S: Shape, E: Unit> LazyTensor<S, E> {
    pub fn load_into_cpu(&mut self, device: &Cpu) {
        let tensor = self.load_on(device);
        *self = Self::CPU(tensor);
    }

    #[cfg(feature = "cuda")]
    pub fn load_into_cuda(&mut self, device: &dfdx::tensor::Cuda) {
        let tensor = self.load_on(device);
        *self = Self::CUDA(tensor);
    }
}

impl<S: Shape, E: Unit> LazyTensor<S, E> {
    fn shape(&self) -> S {
        match self {
            Self::Disk { path: _, shape } => *shape,
            Self::CPU(tensor) => *tensor.shape(),
            #[cfg(feature = "cuda")]
            Self::CUDA(tensor) => *tensor.shape(),
        }
    }

    pub fn load_on<D: ZerosTensor<E> + TensorFromVec<E> + CopySlice<E>>(
        &self,
        device: &D,
    ) -> Tensor<S, E, D> {
        let shape = self.shape();
        let numel = shape.num_elements();

        match self {
            Self::Disk { path, shape } => {
                let mut loaded = device.zeros_like(shape);
                let file = std::fs::File::open(path).unwrap();
                let mmap = unsafe { memmap2::Mmap::map(&file).unwrap() };
                let bytes: &[u8] = &mmap;
                let ptr = bytes.as_ptr() as *const E;
                assert!(bytes.len() < (isize::MAX as usize));
                assert_eq!(bytes.len(), numel * std::mem::size_of::<E>());
                assert_eq!(ptr.align_offset(std::mem::align_of::<E>()), 0);
                // # Safety
                // - assertion checks for byte length
                // - non-null because we created from bytes slice
                // - aligned due to assertion
                let slice = unsafe { std::slice::from_raw_parts(ptr, numel) };
                loaded.copy_from(slice);
                loaded
            }
            Self::CPU(tensor) => {
                if TypeId::of::<D>() == TypeId::of::<Cpu>() {
                    // Here since we know `D` is of type `Cpu`, we can just clone the tensor.
                    // However we can't easily return `tensor.clone()` because of the generic
                    // type.
                    //
                    // One idea might be to use std::mem::transmute, however that gives us
                    // an error about depedendly sized types for some reason.
                    //
                    // Instead we can go through Box<Any> and downcast it, which basically
                    // goes through pointers to do this.
                    let t: Box<dyn Any> = Box::new(tensor.clone());
                    *t.downcast().unwrap()
                } else {
                    let mut loaded = device.zeros_like(tensor.shape());
                    let buf = tensor.as_vec();
                    loaded.copy_from(&buf);
                    loaded
                }
            }
            #[cfg(feature = "cuda")]
            Self::CUDA(tensor) => {
                if TypeId::of::<D>() == TypeId::of::<Cpu>() {
                    // See comment in corresponding Self::CPU branch.
                    let t: Box<dyn Any> = Box::new(tensor.clone());
                    *t.downcast().unwrap()
                } else {
                    let mut loaded = device.zeros_like(tensor.shape());
                    let buf = tensor.as_vec();
                    loaded.copy_from(&buf);
                    loaded
                }
            }
        }
    }
}
