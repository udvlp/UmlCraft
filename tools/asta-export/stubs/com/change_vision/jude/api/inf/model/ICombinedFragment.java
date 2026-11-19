// Compile stub for CI: only the part of Astah's API that AstaExport.java uses, declared
// as the API declares it (see tools/asta-export/README.md).
package com.change_vision.jude.api.inf.model;

public interface ICombinedFragment extends com.change_vision.jude.api.inf.model.INamedElement {
    public abstract com.change_vision.jude.api.inf.model.IInteractionOperand[] getInteractionOperands();
    public abstract void setName(java.lang.String p0) throws com.change_vision.jude.api.inf.exception.InvalidEditingException;
}
